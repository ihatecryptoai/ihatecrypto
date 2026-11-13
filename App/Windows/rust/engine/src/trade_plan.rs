//! A trade plan as the Mac draws it (TradePlan.swift; Studio.swift planRoute, Canvases.swift planBlocker/planFix,
//! TradeAdvice.swift): every message about it redraws its route — FROM, BRIDGE, CHECK RULES, SECOND CHECK, OPEN,
//! PROTECT, SIMULATION, SIGN, RESULT — and the chat checks it item by item. A plan the rules stop stays on the canvas
//! with the one change that passes; one that passes becomes the card waiting for Confirm in its place. Each coin's ATR
//! and the last bridge quote are `State::atr_kept` and `quote_kept`.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use evm::protocols::SponsoredTransfer;
use evm::protocols::batch7702::BatchCall;
use evm::protocols::relay::MulticallPermit;
use evm::protocols::{Permit3009, safe};
use execution::trade_signer::{EvmFees, Signing, TradeError, TradeSigner, TradeWallet};
use fancy_regex::Regex;
use hyperliquid::MsgPack;
use ihatecrypto_core::foundation::{Num, Rounding, double, rounded};
use ihatecrypto_core::guardian::ExitRules;
use ihatecrypto_core::intent::{Fmt, IntentParser, Side, TradeIntent};
use ihatecrypto_core::network::{Amount, Bridge, Hyperliquid, Origin};
use ihatecrypto_core::normalize::{ChatRouter, Route};
use ihatecrypto_core::plan::{Block, BlockStatus, BridgeQuote, Evaluation, Live, Market, MarketGroup, Plan, RiskCalc, Sizing};
use ihatecrypto_core::position_command::PositionCommand;
use ihatecrypto_core::risk::{Level, OverLimit, RiskPolicy, RiskRules};
use ihatecrypto_core::take_style::TakeStyle;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use trade::executor::{Order, Position, Prepared};
use trade::{EvmNetwork, Executor, HyperliquidAccount};
use wallet::WalletError;
use wallet::evm::EvmSignature;

use crate::canvas::{Action, Board, Money, Step};
use crate::engine::{AtrKept, Engine, Pending, QuoteKept, Request};
use crate::from_core::RouterOption;
use crate::moves::PlanReview;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    /// The plan's market and its group, for the picker in its header.
    pub market: String,
    pub group: String,
    /// Why it can't go, as a sentence and in a few words for its status; None when it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    /// A rule stops it (red); false while it only waits for a detail such as the stop (orange).
    #[serde(default)]
    pub blocked: bool,
    /// The one change that passes every rule, the $10 minimum and the money (Plan.fix), or «By the book».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_detail: Option<String>,
    /// The per-trade risk limit is what stops it: Settings › Trading agent can raise it, at the user's own risk.
    #[serde(default)]
    pub over_limit: bool,
    /// «Take profit like the pros»: the three ways, while asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takes: Option<Vec<TakeChoice>>,
    /// What Confirm does, under the route.
    pub footer: String,
    /// Watched addresses: the simulation is shown and nothing can sign here (the Mac's Preview).
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeChoice {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub note: String,
    /// What it pays at the take: «+$2.00», or «no cap».
    pub payoff: String,
    pub recommended: bool,
}

/// The plan's dry bridge quote and the EVM network the USDC leaves from when Solana is short (None: Solana).
#[derive(Debug, Clone)]
pub struct PlanQuote {
    pub quote: BridgeQuote,
    pub from: Option<EvmNetwork>,
}

/// The position board's price ruler (the Mac's PriceRuler): take, entry, now and stop on one line, and the guardian's
/// word under it (PositionBoard). Prices as numbers for the drawing, each label as the card says it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionRuler {
    pub points: Vec<RulerPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard_line: Option<String>,
}

/// `PositionRuler.Point`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RulerPoint {
    pub kind: String,
    pub at: f64,
    pub text: String,
}

/// A plan's board in these states is the plan's own and gives way to its next version; a move that ran stays.
pub const PLAN_STATES: [&str; 3] = ["plan", "waiting", "cancelled"];

/// The picker's groups in Russian.
pub fn group_ru(g: MarketGroup) -> &'static str {
    match g { MarketGroup::Crypto => "крипта", MarketGroup::Commodities => "сырьё", MarketGroup::Stocks => "акции", MarketGroup::Indices => "индексы и валюты" }
}

/// Signs nothing: a plan for watched addresses is prepared and simulated, as the Mac's Preview, and stops there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSigner {
    pub refusal: String,
}

impl PreviewSigner {
    fn refuse<T: Send + 'static>(&self) -> Signing<'_, T> {
        let refusal = self.refusal.clone();
        Box::pin(async move { Err(TradeError::Failed(refusal)) })
    }
}

impl TradeSigner for PreviewSigner {
    fn sign_solana(&self, _: Vec<u8>, _: Vec<String>, _: Vec<String>, _: BTreeMap<String, String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_hyperliquid(&self, _: MsgPack, _: u64) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_hyperliquid_user(&self, _: MsgPack, _: Vec<String>) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_relay_mapping(&self, _: String, _: String, _: Vec<u8>, _: u64) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_evm(&self, _: EvmFees, _: String, _: Vec<u8>, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_near_intent_deposit(&self, _: u128, _: String, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { self.refuse() }
    // The rest refuse too, as their defaults do; spelled out so a preview never signs by a default changing.
    fn sign_evm_batch(&self, _: EvmFees, _: Vec<BatchCall>, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_evm_payable(&self, _: EvmFees, _: String, _: u64, _: Vec<u8>, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_evm_transfer(&self, _: EvmFees, _: String, _: u64, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_near_transfer(&self, _: u128, _: String, _: Vec<String>, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_near_delete_account(&self, _: String, _: Vec<String>, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_safe_move(&self, _: safe::Move, _: Vec<String>) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_solana_sponsored(&self, _: Vec<u8>, _: Vec<String>, _: BTreeMap<String, String>, _: String) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_receive_authorization(&self, _: Permit3009) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_relay_multicall_permit(&self, _: MulticallPermit) -> Signing<'_, Vec<EvmSignature>> { self.refuse() }
    fn sign_sponsored_transfer(&self, _: SponsoredTransfer, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
}

// MARK: Numbers said on the plan

/// A checked number as `Fmt` says it; «NaN» where Swift's Decimal would have gone NaN or past range.
fn usd(n: Num) -> String { n.0.map_or("$NaN".into(), Fmt::usd) }
fn pct(n: Num) -> String { n.0.map_or("NaN%".into(), Fmt::pct) }
fn num(n: Num) -> String { n.0.map_or("NaN".into(), |d| Fmt::num(d, 2)) }
fn price(n: Num) -> String { n.0.map_or("NaN".into(), Fmt::price) }

/// Swift's `dropLast()` of a line: its last character (the period) gone.
fn drop_last(s: &str) -> String {
    let mut c = s.chars();
    c.next_back();
    c.as_str().into()
}

/// The first and last characters of an address (Swift's `prefix`, `suffix`).
fn head(s: &str, n: usize) -> String { s.chars().take(n).collect() }
fn tail(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

/// Rows are «label\tvalue»: the eye finds the number without reading a sentence.
fn row(label: &str, value: &str) -> String { format!("{label}\t{value}") }

/// A route card's status from its block's.
fn state(s: BlockStatus, check: bool) -> String {
    match s {
        BlockStatus::Blocked => "blocked",
        BlockStatus::NeedsInput => "attention",
        BlockStatus::Ready | BlockStatus::Warning | BlockStatus::Info => if check { "passed" } else { "info" },
        BlockStatus::NoData => "info",
    }.into()
}

fn step(label: &str, title: &str, lines: Vec<String>, money: Option<Money>, status: &str) -> Step {
    Step { label: label.into(), title: title.into(), lines, money, status: Some(status.into()) }
}

/// `orderType ?? ""`.
fn kind(o: &Order) -> &str { o.order_type.as_deref().unwrap_or("") }

/// «12 s» in a bridge's time.
static SECONDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\s*s\b").expect("a regex"));

impl Engine {
    // MARK: The numbers the plan is checked with

    /// ATR(14) of the 4-hour candles, for a stop by the book: kept ten minutes, asked again when Hyperliquid refuses
    /// (it does when many readers ask at once).
    pub async fn plan_atr(&self, coin: &str) -> Option<Decimal> {
        let kept = self.st().atr_kept.get(coin).cloned();
        if let Some(kept) = kept && (Utc::now() - kept.at).num_milliseconds() < 600_000 { return Some(kept.pct) }
        let v = Self::retried(|| Hyperliquid::atr_pct(coin, Utc::now())).await;
        if let Some(v) = v { self.st_mut().atr_kept.insert(coin.into(), AtrKept { pct: v, at: Utc::now() }); }
        v
    }

    /// Hyperliquid's markets, read once and asked again on a refusal, for the plan and its picker.
    pub async fn plan_markets_loaded(&self) {
        let empty = self.st().markets.is_empty();
        if empty && let Some(read) = Self::retried(Hyperliquid::markets).await { self.st_mut().markets = read }
    }

    /// Up to three tries while the refusals come back quickly; a request that hung its whole timeout isn't asked again,
    /// so the chat never waits more than one timeout and a few seconds.
    pub async fn retried<T, E, F, Fut>(read: F) -> Option<T>
    where F: Fn() -> Fut, Fut: Future<Output = Result<T, E>> {
        let start = Instant::now();
        for attempt in 1..=3u64 {
            if let Ok(v) = read().await { return Some(v) }
            if attempt >= 3 || start.elapsed() >= Duration::from_secs(8) { break }
            tokio::time::sleep(Duration::from_millis(400 * attempt)).await;
        }
        None
    }

    /// What the positions open now lose at their stops; None when one has none.
    fn open_risk(&self) -> Option<Decimal> {
        let st = self.st();
        HyperliquidAccount::risk_at_stops(&st.positions, &st.hl_orders, &HashMap::new())
    }

    /// What's free on Hyperliquid for a plan: the main balance, and a HIP-3 market's own exchange's as well.
    fn hl_withdrawable(&self, market: Option<&Market>) -> Option<Decimal> {
        let st = self.st();
        st.place_facts.hyperliquid.as_ref().and_then(|hl| {
            let dex = market.and_then(|m| m.dex.as_ref()).and_then(|d| hl.dexes.iter().find(|x| &x.dex == d)).map_or(Decimal::ZERO, |x| x.withdrawable);
            (Num::of(hl.withdrawable) + dex).0
        })
    }

    /// What Plan.blocks reads besides the plan: the balances, the market, the open risk, the rules.
    pub fn plan_live(&self, market: Option<&Market>, notes: Vec<String>, quoted: Option<&PlanQuote>) -> Live {
        let (solana_set, hl_set, loaded) = { let st = self.st(); (st.solana_address().is_some(), st.evm_address().is_some(), !st.markets.is_empty()) };
        Live {
            quote: quoted.map(|q| q.quote.clone()),
            from: quoted.and_then(|q| q.from.as_ref()).map(|n| (n.name.to_string(), self.usdc_free(n.key).unwrap_or_default())),
            equity: self.rule_equity(),
            policy: self.policy(),
            open_risk: self.open_risk(),
            solana_address_set: solana_set,
            solana_usdc: self.usdc_free("solana"),
            hl_address_set: hl_set,
            // A HIP-3 market can use what is free on its own exchange as well as the main balance.
            hl_withdrawable: self.hl_withdrawable(market),
            markets_loaded: loaded,
            market: market.cloned(),
            notes,
            ..Live::default()
        }
    }

    /// The bridge as the plan would take it, quoted dry — no address is sent (the Mac's quoteIfNeeded): what goes over,
    /// what arrives, the fee, and the EVM network the USDC leaves from when Solana is short. Kept a minute for the same
    /// amount and the same USDC on Solana; None when nothing needs bridging or no bridge answered.
    pub async fn plan_quote(&self, r: &TradeIntent, e: &Evaluation, market: Option<&Market>) -> Option<PlanQuote> {
        let solana = self.usdc_free("solana");
        let amount = if let Some(a) = r.amount_usdc { Amount::Send(a) } else {
            let need = Plan::bridge_need(r, e, self.hl_withdrawable(market), market.map(|m| m.mark_px))?;
            Amount::Arrive(Executor::bridge_arrive(need, e.sizing.as_ref().map_or(Decimal::ZERO, |s| s.margin), solana))
        };
        let one_click = self.st().open_via_1click == Some(true);
        let key = format!("{amount:?} {} {one_click}", solana.unwrap_or_default());
        let kept = self.st().quote_kept.clone();
        if let Some(kept) = kept && kept.key == key && (Utc::now() - kept.at).num_milliseconds() < 60_000 { return Some(kept.quoted) }
        // A new Hyperliquid account pays a one-time opening fee on its first deposit; the plan shows it up front.
        let evm = self.st().evm_address();
        let new_account = match evm { Some(evm) => Hyperliquid::is_new_account(&evm).await.unwrap_or(false), None => false };
        let mut q = Bridge::dry_quote(amount, new_account, None, new_account && one_click).await.ok()?;
        // The executor picks the same way, from the same quote.
        let net = if q.amount_in > solana.unwrap_or_default() { EvmNetwork::funding(q.amount_in, |k| self.usdc_free(k)) } else { None };
        if let Some(net) = net {
            let origin = Origin { chain: net.relay_chain, usdc: net.usdc.into(), decimals: net.usdc_decimals };
            if let Ok(other) = Bridge::dry_quote(Amount::Send(q.amount_in), new_account, Some(&origin), false).await { q = other }
        }
        let quoted = PlanQuote { quote: q, from: net.cloned() };
        self.st_mut().quote_kept = Some(QuoteKept { key, quoted: quoted.clone(), at: Utc::now() });
        Some(quoted)
    }

    /// Why a plan can't go, in a few words for its status and a sentence for the chat (Canvases.swift planBlocker); None
    /// when nothing stops it or a detail is still missing (that has its own «By the book»). `from`: «Solana» unless the
    /// money leaves an EVM network.
    pub fn plan_blocker(&self, e: &Evaluation, blocks: &[Block], equity: Option<Decimal>, from: &str) -> Option<(String, String)> {
        if !e.missing.is_empty() { return None }
        if let Some(s) = &e.sizing && s.notional < Decimal::TEN && e.problem.is_some() {
            return Some((self.say("order under the $10 minimum", "ордер меньше $10"),
                         self.say(&format!("The order is {}, under Hyperliquid’s $10 minimum.", Fmt::usd(s.notional)),
                                  &format!("Ордер {} — меньше минимума Hyperliquid $10.", Fmt::usd(s.notional)))));
        }
        if let Some(p) = &e.problem { return Some((self.say("the position can’t be placed", "позицию не выставить"), p.clone())) }
        if let Some(source) = blocks.iter().find(|b| b.id == "source") && source.status == BlockStatus::Blocked && let Some(last) = source.lines.last() {
            let short = drop_last(last);
            return Some((self.say("not enough USDC", "не хватает USDC"),
                         self.say(&format!("{short} on {from}."), &format!("{} на {from}.", short.replace("Short by", "Не хватает")))));
        }
        let policy = self.policy();
        if policy.limit_mode() == OverLimit::Block && let Some(s) = &e.sizing && let Some(equity) = equity && equity > Decimal::ZERO
            && (Num::of(s.loss_at_stop_usd) / equity * 100).0.is_some_and(|x| x > policy.max_risk_pct) {
            let (max, loss, limit) = (Fmt::pct(policy.max_risk_pct), Fmt::usd(s.loss_at_stop_usd), usd(Num::of(equity) * policy.max_risk_pct / 100));
            return Some((self.say(&format!("risk over the {max} limit"), &format!("риск выше лимита {max}")),
                         self.say(&format!("At the stop {loss} is lost, over the limit of {max} of capital ({limit})."),
                                  &format!("На стопе теряется {loss} — больше лимита {max} капитала ({limit})."))));
        }
        let stopped = blocks.iter().find(|b| b.status == BlockStatus::Blocked)?;
        let line = stopped.lines.iter().find(|l| l.starts_with('✗')).map(|l| l.chars().skip(2).collect())
            .or_else(|| stopped.lines.first().cloned()).unwrap_or_else(|| stopped.title.clone());
        Some((self.say("a rule stops it", "его стопорит правило"), line))
    }

    /// For a plan the rules stop: the one change that passes every rule, the $10 minimum and the money (Plan.fix), said as
    /// what changes in the trade: its button's title, the detail under it and the command it puts in the chat.
    pub fn plan_fix(&self, raw: &TradeIntent, m: &Market, now: Option<&Sizing>) -> Option<(String, String, String)> {
        // ponytail: Solana's USDC (or an EVM network's, the richest) less a bridge-fee cushion, plus what's free on
        // Hyperliquid, as the Mac counts it; Confirm's preparation quotes it exactly.
        let keys: Vec<String> = self.st().portfolio.networks.iter().filter(|n| n.key != "hyperliquid").map(|n| n.key.clone()).collect();
        let richest = keys.iter().filter_map(|k| self.usdc_free(k)).max().unwrap_or_default();
        let free = self.hl_withdrawable(Some(m)).unwrap_or_default();
        let cushioned = (Num::of(richest) - Decimal::new(1, 1)).0.map_or(Decimal::ZERO, |x| x.max(Decimal::ZERO));
        let available = (Num::of(cushioned) + free).0.unwrap_or(Decimal::ZERO);
        let (command, s) = Plan::fix(raw, m, self.rule_equity(), &self.policy(), self.open_risk(), Some(available))?;
        let title = if command.ends_with("USDC") {
            self.say(&format!("Put {command} in the trade"), &format!("Вложить {command} в сделку"))
        } else if now.is_none_or(|n| s.notional > n.notional) {
            self.say(&format!("Raise the order to {}", Fmt::usd(s.notional)), &format!("Поднять ордер до {}", Fmt::usd(s.notional)))
        } else {
            self.say(&format!("Cut the order to {}", Fmt::usd(s.notional)), &format!("Уменьшить ордер до {}", Fmt::usd(s.notional)))
        };
        let was = now.map(|n| format!(" {} −{}", self.say("instead of", "вместо"), Fmt::usd(n.loss_at_stop_usd))).unwrap_or_default();
        let (loss, margin) = (Fmt::usd(s.loss_at_stop_usd), Fmt::usd(s.margin));
        Some((title, self.say(&format!("At the stop −{loss}{was}, margin {margin} — passes every rule."),
                              &format!("На стопе −{loss}{was}, маржа {margin} — проходит все правила.")), command))
    }

    // MARK: The checklist

    /// The plan checked item by item, the way its route checks it (TradeAdvice.planChecklist): market, direction, risk,
    /// stop, take, entry, size, money, the rules and the result, then the one next step. Lines start with ✓ ✗ ⚠ ·, and
    /// the chat draws them as a checklist card. `ready`: the card waits for Confirm. `failed`: the preparation's
    /// refusal, said in the next step's place.
    #[allow(clippy::too_many_arguments)]
    pub fn plan_checklist(&self, asked: &TradeIntent, r: &TradeIntent, e: &Evaluation, blocks: &[Block], market: Option<&Market>, atr_read: bool, ready: bool,
                          quoted: Option<&PlanQuote>, failed: Option<&str>) -> String {
        let say = |en: &str, ru: &str| self.say(en, ru);
        let (sizing, equity, policy) = (e.sizing.as_ref(), self.rule_equity(), self.policy());
        let mut trial = asked.clone();
        trial.auto_rules = Some(true);
        let book = Plan::resolve(&trial, market, equity, &policy, None).0;
        let share = |v: Decimal| -> String {
            match equity {
                Some(eq) if eq > Decimal::ZERO => {
                    let p = pct((Num::of(v) / eq * 100).rounded(1, Rounding::Plain));
                    say(&format!(" ({p} of capital)"), &format!(" ({p} капитала)"))
                }
                _ => String::new(),
            }
        };
        let side = if r.side == Side::Long { say("long", "лонг") } else { say("short", "шорт") };
        let name = IntentParser::pretty(&r.asset);
        let Some(m) = market else {
            return say(&format!("✗ Market: {name} has no price yet\n· Direction: {side}\n· The rest is checked once the price loads."),
                       &format!("✗ Рынок: у {name} пока нет цены\n· Направление: {side}\n· Остальное проверю, когда загрузится цена."));
        };
        let mut out: Vec<String> = Vec::new();
        let entry = r.entry_px.unwrap_or(m.mark_px);
        out.push(say("✓ Market: ", "✓ Рынок: ") + &format!("{name} {}", Fmt::price(m.mark_px))
                 + &m.change24h().map(|c| format!(", 24h {}{}", if c >= Decimal::ZERO { "+" } else { "" }, Fmt::pct(rounded(c, 2, Rounding::Plain)))).unwrap_or_default());
        out.push(say("✓ Direction: ", "✓ Направление: ") + &side);
        if let Some(risk) = r.risk_usd {
            let from = if asked.risk_usd.is_some() { String::new() } else {
                asked.amount_usdc.map_or_else(|| say(" by the book", " по правилам"),
                                              |a| say(&format!(" from {} USDC in the trade", Fmt::num(a, 2)), &format!(" из {} USDC в сделке", Fmt::num(a, 2))))
            };
            out.push(say("✓ Risk: ", "✓ Риск: ") + &Fmt::usd(risk) + &share(risk) + &from);
        } else {
            let (b, p) = (book.risk_usd.map(Fmt::usd), Fmt::pct(policy.default_risk_pct));
            out.push(say("✗ Risk: not set", "✗ Риск: не задан")
                     + &b.map(|b| say(&format!(" — by the book {b}, {p} of capital"), &format!(" — по правилам {b}, {p} капитала"))).unwrap_or_default());
        }
        if let Some(stop) = r.stop_pct {
            let direction: i64 = if r.side == Side::Short { 1 } else { -1 };
            let at = price(sizing.map_or_else(|| Num::of(entry) * (1 + direction * Num::of(stop) / 100), |s| Num::of(s.stop_price)));
            out.push(say(&format!("✓ Stop: {} at {at}", Fmt::pct(stop)), &format!("✓ Стоп: {} на {at}", Fmt::pct(stop)))
                     + &if asked.stop_pct.is_none() { say(" by the book", " по правилам") } else { String::new() });
        } else if let Some(b) = book.stop_pct {
            let (b, x) = (Fmt::pct(b), Fmt::num(policy.atr_stop_multiple, 2));
            out.push(say(&format!("✗ Stop: not set — by the book {b}, {x} × 4h volatility"), &format!("✗ Стоп: не задан — по правилам {b}, {x} × волатильность за 4 часа")));
        } else {
            out.push(if atr_read { say("✗ Stop: not set", "✗ Стоп: не задан") } else {
                say("✗ Stop: not set — Hyperliquid’s 4h candles didn’t load, so the book can’t size it. Say your own, e.g. “stop 3%”",
                    "✗ Стоп: не задан — свечи Hyperliquid за 4 часа не загрузились, поэтому правила его не посчитают. Скажи свой, например «стоп 3%»")
            });
        }
        if let Some(take) = r.take_pct {
            out.push(say(&format!("✓ Take: {}", Fmt::pct(take)), &format!("✓ Тейк: {}", Fmt::pct(take)))
                     + &sizing.and_then(|s| s.take_price).map(|tp| say(&format!(" at {}", Fmt::price(tp)), &format!(" на {}", Fmt::price(tp)))).unwrap_or_default()
                     + &if r.partial_take == Some(true) { say(", half at 1R", ", половина на 1R") } else { String::new() });
        } else {
            let rr = Fmt::num(policy.default_reward_risk, 2);
            out.push(say(&format!("· Take: none — by the book {rr}R"), &format!("· Тейк: нет — по правилам {rr}R")));
        }
        out.push(match (r.twap_minutes, r.entry_px) {
            (Some(minutes), _) => {
                let (time, ru) = (TradeIntent::duration(minutes), Self::time_ru(&TradeIntent::duration(minutes)));
                if self.agent_active() {
                    let (parts, parts_ru) = if r.twap_randomize == Some(true) { ("randomized parts", "части случайного размера") } else { ("equal parts", "равные части") };
                    say(&format!("✓ Entry: TWAP over {time} on Hyperliquid, {parts}, no ihatecrypto fee on it"),
                        &format!("✓ Вход: TWAP за {ru} на Hyperliquid, {parts_ru}, без комиссии ihatecrypto"))
                } else {
                    say(&format!("✗ Entry: TWAP over {time} on Hyperliquid needs the trading key on, so ihatecrypto can stop it while you're away"),
                        &format!("✗ Вход: TWAP за {ru} на Hyperliquid требует включённого торгового ключа, чтобы ihatecrypto мог остановить его без тебя"))
                }
            }
            (None, Some(px)) => say(&format!("✓ Entry: limit {}", Fmt::price(px)), &format!("✓ Вход: лимит {}", Fmt::price(px)))
                + &r.pullback_pct.map(|p| say(&format!(" ({} pullback)", Fmt::pct(p)), &format!(" (откат {})", Fmt::pct(p)))).unwrap_or_default(),
            (None, None) => say("✓ Entry: at market", "✓ Вход: по рынку"),
        });
        if let Some(s) = sizing {
            out.push(format!("{} ", if e.problem.is_none() { "✓" } else { "✗" }) + &say("Size: ", "Размер: ")
                     + &format!("{} · {}× · ", Fmt::usd(s.notional), Fmt::num(r.leverage.unwrap_or(Decimal::ONE), 2))
                     + &say("margin", "маржа") + &format!(" {}", Fmt::usd(s.margin)) + &e.problem.as_ref().map(|p| format!(". {p}")).unwrap_or_default());
            let (source, bridge) = (blocks.iter().find(|b| b.id == "source"), blocks.iter().find(|b| b.id == "bridge"));
            let free = self.st().place_facts.hyperliquid.as_ref().map_or_else(|| "—".to_string(), |h| Fmt::usd(h.withdrawable));
            let from = quoted.and_then(|q| q.from.as_ref());
            let place = from.map_or("Solana", |n| n.name);
            if let Some(source) = source && source.status == BlockStatus::Blocked && let Some(short) = source.lines.last() {
                let short = drop_last(short);
                out.push(say(&format!("✗ Money: {short} on {place}."), &format!("✗ Деньги: {} на {place}.", short.replace("Short by", "не хватает"))));
            } else if let Some(bridge) = bridge && matches!(bridge.status, BlockStatus::Ready | BlockStatus::Warning) && let Some(q) = quoted.map(|q| &q.quote) {
                let (sent, arrive, provider) = (Fmt::num(q.amount_in, 2), Fmt::num(q.amount_out, 2), &q.provider);
                let straight = if from.is_none() { String::new() } else {
                    let only = Fmt::num(self.usdc_free("solana").unwrap_or_default(), 2);
                    say(&format!(", straight: Solana has only {only}"), &format!(", напрямую: на Solana только {only}"))
                };
                out.push(say(&format!("✓ Money: {free} free on Hyperliquid + {sent} USDC from {place} over {provider} (≈ {arrive} arrive)"),
                             &format!("✓ Деньги: {free} свободно на Hyperliquid + {sent} USDC с {place} через {provider} (придёт ≈ {arrive})")) + &straight);
                // The bridge's warning (it costs more than the take earns) belongs next to the money.
                if bridge.status == BlockStatus::Warning && let Some(costly) = bridge.lines.iter().find(|l| l.starts_with("The bridge costs more")) {
                    out.push("⚠ ".to_string() + &say("Money: ", "Деньги: ") + costly);
                }
            } else if bridge.is_some_and(|b| b.status == BlockStatus::Info) {
                out.push(say(&format!("✓ Money: {free} free on Hyperliquid covers the margin"), &format!("✓ Деньги: {free} свободно на Hyperliquid — маржи хватает")));
            } else if let Some(bridge) = bridge && bridge.status == BlockStatus::Blocked && let Some(why) = bridge.lines.first() {
                out.push(say("✗ Money: ", "✗ Деньги: ") + why);
            } else {
                out.push(say(&format!("· Money: {free} free on Hyperliquid, the rest comes from Solana over Relay, quoted on the card"),
                             &format!("· Деньги: {free} свободно на Hyperliquid, остальное придёт с Solana через Relay, цена — на карточке")));
            }
            let advice = RiskRules::check(r, s, m, equity, &policy, self.open_risk());
            let problems: Vec<_> = advice.iter().filter(|a| a.level != Level::Ok).collect();
            for a in &problems { out.push(format!("{} ", if a.level == Level::Block { "✗" } else { "⚠" }) + &say("Rules: ", "Правила: ") + &say(&a.text, &a.ru)) }
            if problems.is_empty() { out.push(say("✓ Rules: all met", "✓ Правила: все соблюдены")) }
            let loss = Fmt::usd(s.loss_at_stop_usd);
            let mut result = say(&format!("✓ Result: at the stop −{loss}"), &format!("✓ Итог: на стопе −{loss}")) + &share(s.loss_at_stop_usd);
            if let Some(p) = s.profit_at_take_usd && s.loss_at_stop_usd > Decimal::ZERO {
                let rr = num((Num::of(p) / s.loss_at_stop_usd).rounded(1, Rounding::Plain));
                result += &say(&format!(", at the take +{}, 1:{rr}", Fmt::usd(p)), &format!(", на тейке +{}, 1:{rr}", Fmt::usd(p)));
            }
            out.push(result);
        } else {
            out.push(say("· Size, money and rules: checked once the risk and the stop are set", "· Размер, деньги и правила: проверю, когда будут риск и стоп"));
        }
        let next = if !e.missing.is_empty() {
            let (en, ru) = Self::missing_example(&e.missing);
            if asked.auto_rules == Some(true) { say(&format!("Say what’s missing, e.g. “{en}”."), &format!("Скажи, чего не хватает, например «{ru}».")) }
            else { say("Press “By the book” or say your own, e.g. “risk $1 stop 3%”.", "Нажми «По правилам» или скажи свои, например «риск $1 стоп 3%».") }
        } else if out.iter().any(|l| l.starts_with('✗')) {
            say("Fix the ✗ above — for example a smaller risk or a wider stop — and I'll check again.", "Исправь ✗ выше — например, риск поменьше или стоп пошире, — и я проверю снова.")
        } else if let Some(failed) = failed {
            let why = if failed.ends_with('.') { failed.to_string() } else { format!("{failed}.") };
            say(&format!("Can't open it: {why} Nothing was signed. Ask again in a moment."), &format!("Не получится открыть: {why} Ничего не подписано, попробуй ещё раз чуть позже."))
        } else if ready {
            say("Ready: the card is on the canvas with its simulation — press Confirm, or change it in words.", "Готово: карточка на холсте вместе с симуляцией — нажми «Подтвердить» или измени словами.")
        } else {
            say("Ready to check: the simulation runs next, before anything is signed.", "Можно проверять: дальше симуляция, до любой подписи.")
        };
        out.join("\n") + "\n" + &next
    }

    // MARK: The route

    /// The trade plan as route cards, in the order it happens (Workspace.planRoute): the money from Solana (or already on
    /// Hyperliquid), the bridge, the rules and the second check, the position, its stop and take, the simulation, the
    /// signatures, and what it comes to. A step a rule stops, or that waits for a detail, says so on its card. Returns the
    /// steps and the footer.
    #[allow(clippy::too_many_arguments)]
    pub fn trade_route(&self, r: &TradeIntent, e: &Evaluation, blocks: &[Block], market: Option<&Market>, p: Option<&Prepared>, review: Option<&str>,
                       reviewed: bool, not_checked: Option<&str>, quoted: Option<&PlanQuote>) -> (Vec<Step>, String) {
        let say = |en: &str, ru: &str| self.say(en, ru);
        let block = |id: &str| blocks.iter().find(|b| b.id == id);
        let sizing = e.sizing.as_ref();
        let pretty = IntentParser::pretty(&r.asset);
        let name = pretty.split(" (").next().unwrap_or(&r.asset).to_string();
        // A price alert tied to the plan comes first: the plan waits for that price (alerts.rs).
        let mut steps: Vec<Step> = self.plan_alert_step().into_iter().collect();
        let (free, ledger, watching, evm) = {
            let st = self.st();
            (st.place_facts.hyperliquid.as_ref().map_or(Decimal::ZERO, |h| h.withdrawable), st.ledger_active(), st.watching(), st.evm_address())
        };
        // «Not needed»: the margin is on Hyperliquid already, and the route starts there.
        let bridge = block("bridge").filter(|b| b.status != BlockStatus::Info);
        if let Some(source) = block("source") {
            if let Some(bridge) = bridge && p.is_none_or(|p| p.bridge.is_some() || p.fund_from.is_some() || p.open_via_1click.is_some()) {
                // What goes over: the prepared bridge's, the amount asked for, or what the margin and fees come to.
                let need = sizing.and_then(|s| (Num::of(s.margin) + s.fees_usd - free).0).map(|x| rounded(x.max(Decimal::ZERO), 2, Rounding::Up));
                let q = if p.is_none() { quoted.map(|q| &q.quote) } else { None };
                let send = p.and_then(|p| p.bridge.as_ref().map(|b| b.spend).or(p.fund_from.as_ref().map(|f| f.1))
                                          .or(p.open_via_1click.map(|o| Decimal::from(o.0) / Decimal::from(1_000_000))))
                    .or(q.map(|q| q.amount_in)).or(r.amount_usdc).or(need);
                let approx = if p.is_none() && q.is_none() && r.amount_usdc.is_none() { "≈ " } else { "" };
                // Solana short: the USDC leaves straight from the EVM network that has it.
                let net: Option<&EvmNetwork> = match p { Some(p) => p.fund_from.as_ref().map(|f| &f.0), None => quoted.and_then(|q| q.from.as_ref()) };
                let from = net.map_or("Solana", |n| n.name);
                let have = self.usdc_free(net.map_or("solana", |n| n.key));
                let money = have.zip(send).map(|(h, s)| Money {
                    place: from.into(), symbol: "USDC".into(), was: Fmt::num(h, 2),
                    after: format!("{approx}{}", num(Num((Num::of(h) - s).0.map(|x| x.max(Decimal::ZERO))))),
                });
                let mut lines: Vec<String> = if money.is_none() { have.map_or_else(|| source.lines.clone(), |h| vec![row(&say("wallet", "кошелёк"), &Fmt::num(h, 2))]) } else { vec![] };
                if source.status == BlockStatus::Blocked && let Some(short) = source.lines.last() {
                    lines.insert(0, say(short, &short.replace("Short by", "Не хватает")));
                }
                steps.push(step(&format!("FROM · {}", from.to_uppercase()), &send.map_or_else(|| "USDC".into(), |s| format!("{approx}{} USDC", Fmt::num(s, 2))),
                                lines, money, &state(source.status, false)));
                let mut lines2: Vec<String>;
                if let Some(b) = p.and_then(|p| p.bridge.as_ref()) {
                    lines2 = vec![row(&say("sends", "отправит"), &format!("{} USDC", Fmt::num(b.spend, 2))), row(&say("arrives", "придёт"), &format!("{} USDC", Fmt::num(b.arriving, 2))),
                                  row(&say("fee", "комиссия"), &b.fees_usd)];
                    if b.opening > Decimal::ZERO { lines2.push(say("incl. Hyperliquid’s one-time account fee", "вкл. разовую плату Hyperliquid за открытие счёта")) }
                } else if let Some((units, arriving)) = p.and_then(|p| p.open_via_1click) {
                    lines2 = vec![row(&say("sends", "отправит"), &format!("{} USDC", Fmt::num(Decimal::from(units) / Decimal::from(1_000_000), 2))),
                                  row(&say("arrives", "придёт"), &format!("{} USDC", Fmt::num(arriving, 2))),
                                  say("NEAR Intents opens the account without its fee, in 2–4 min", "NEAR Intents открывает счёт без платы, за 2–4 мин")];
                } else if let Some((net, usdc)) = p.and_then(|p| p.fund_from.as_ref()) {
                    lines2 = vec![row(&say("sends", "отправит"), &format!("{} USDC", Fmt::num(*usdc, 2))),
                                  say(&format!("straight from {}: Solana is short", net.name), &format!("прямо из {}: на Solana не хватает", net.name))];
                } else if let Some(q) = q {
                    let time = SECONDS.find(&q.time).ok().flatten().map(|m| format!(" · {}", m.as_str())).unwrap_or_default();
                    lines2 = vec![row(&say("sends", "отправит"), &format!("{} USDC", Fmt::num(q.amount_in, 2))), row(&say("arrives", "придёт"), &format!("{} USDC", Fmt::num(q.amount_out, 2))),
                                  row(&say("fee", "комиссия"), &(usd(Num::of(q.amount_in) - q.reserve - q.amount_out) + &time))];
                    if q.opening {
                        lines2.push(if q.provider == "Relay" { say("incl. Hyperliquid’s one-time account fee", "вкл. разовую плату Hyperliquid за открытие счёта") }
                                    else { say("opens the account, no opening fee", "открывает счёт без платы") });
                    }
                } else {
                    lines2 = send.map(|s| vec![row(&say("sends", "отправит"), &format!("≈ {} USDC", Fmt::num(s, 2)))]).unwrap_or_default();
                    lines2.push(say("quoted once the position passes the checks", "цена — когда позиция пройдёт проверки"));
                }
                let lands = p.and_then(|p| p.bridge.as_ref().map(|b| b.arriving).or(p.open_via_1click.map(|o| o.1))).or(q.map(|q| q.amount_out)).or(send);
                let one_click = p.is_some_and(|p| p.open_via_1click.is_some()) || q.is_some_and(|q| q.provider != "Relay");
                let landed = lands.map(|l| Money {
                    place: "Hyperliquid".into(), symbol: "USDC".into(), was: Fmt::num(free, 2),
                    after: format!("{}{}", if p.is_none() && q.is_none() { "≈ " } else { "" }, num(Num::of(free) + l)),
                });
                steps.push(step(if one_click { "BRIDGE · 1CLICK" } else { "BRIDGE · RELAY" }, "→ Hyperliquid", lines2, landed,
                                &if bridge.status == BlockStatus::NeedsInput { "info".into() } else { state(bridge.status, false) }));
            } else {
                steps.push(step("FROM · HYPERLIQUID", &format!("{} USDC", Fmt::num(free, 2)), vec![say("the margin is on Hyperliquid already", "маржа уже на Hyperliquid")], None, "info"));
            }
        }
        if let Some(risk) = block("risk") {
            let mut lines: Vec<String> = Vec::new();
            if let Some(s) = sizing {
                let policy = self.policy();
                let share = self.rule_equity().filter(|eq| *eq > Decimal::ZERO).map(|eq| num((Num::of(s.loss_at_stop_usd) / eq * 100).rounded(1, Rounding::Plain)));
                lines.push(row(&say("risk", "риск"), &(Fmt::usd(s.loss_at_stop_usd) + &share.map(|p| format!(" · {p}%")).unwrap_or_default())));
                let (max, max_ru) = (RiskPolicy::limit(policy.max_risk_pct, false), RiskPolicy::limit(policy.max_risk_pct, true));
                lines.push(row(&say("limit", "лимит"), &say(&format!("{max} a trade"), &format!("{max_ru} на сделку"))));
                if let Some(profit) = s.profit_at_take_usd && s.loss_at_stop_usd > Decimal::ZERO {
                    lines.push(row(&say("reward/risk", "прибыль/риск"), &format!("1:{}", num((Num::of(profit) / s.loss_at_stop_usd).rounded(1, Rounding::Plain)))));
                }
            }
            // A rule it breaks, in its own words, first.
            let broken: Vec<String> = risk.lines.iter().filter(|l| l.starts_with('✗')).cloned().collect();
            lines.splice(0..0, broken);
            steps.push(step("CHECK RULES", &risk.title, if lines.is_empty() { risk.lines.clone() } else { lines }, None, &state(risk.status, true)));
        }
        if let Some(not_checked) = not_checked && p.is_some() {
            // It couldn't read the plan: the route says so, rather than the check passing or silently missing.
            steps.push(step("SECOND CHECK", &say("Not checking", "Не проверяю"), vec![not_checked.into()], None, "info"));
        } else if reviewed && p.is_some() {
            let mut lines = vec![review.map_or_else(|| say("Matches your request.", "Совпадает с твоей просьбой."), String::from)];
            if p.is_some_and(|p| p.bridge.is_some()) {
                lines.push(say("USDC: Circle's token on Solana, EPjF…Dt1v.", "USDC: токен Circle на Solana, EPjF…Dt1v."));
                lines.push(say("Bridge: Relay, its program on the allowed list. Each signature is checked again at Confirm.",
                               "Мост: Relay, его программа в списке разрешённых. Каждая подпись проверяется снова при «Подтвердить»."));
            }
            if let Some(evm) = &evm {
                let (a, b) = (head(evm, 6), tail(evm, 4));
                lines.push(say(&format!("To: your own Hyperliquid account {a}…{b}."), &format!("Куда: твой собственный счёт Hyperliquid {a}…{b}.")));
            }
            lines.push(say(&format!("Market: {name} on Hyperliquid."), &format!("Рынок: {name} на Hyperliquid.")));
            steps.push(step("SECOND CHECK", &if review.is_none() { say("All correct", "Всё верно") } else { say("Has doubts", "Есть сомнения") },
                            lines, None, if review.is_none() { "passed" } else { "attention" }));
        }
        if let Some(position) = block("position") {
            let leverage = format!("{}×", Fmt::num(r.leverage.unwrap_or(Decimal::ONE), 2));
            let mut lines = position.lines.clone();
            if let (Some(s), Some(m)) = (sizing, market) {
                lines = vec![row(&say("amount", "объём"), &format!("{} {name}", Fmt::num(s.size, u32::try_from(m.sz_decimals).unwrap_or(0)))),
                             row(&if r.entry_px.is_none() { say("entry", "вход") } else { say("limit", "лимит") }, &format!("{}{}", if r.entry_px.is_none() { "≈ " } else { "" }, Fmt::price(s.entry))),
                             row(&say("margin", "маржа"), &Fmt::usd(s.margin)),
                             row(&say("liquidation", "ликвидация"), &format!("≈ {}", Fmt::price(s.liquidation_price)))];
            }
            // What stops it goes first, in red. Unsized, the block's lines are already the problem: once is enough (c308a92).
            if position.status == BlockStatus::Blocked && let Some(problem) = e.problem.clone().or_else(|| position.lines.last().cloned())
                && !lines.contains(&problem) { lines.insert(0, problem) }
            steps.push(step(&format!("OPEN {} · {}", if r.side == Side::Long { "LONG" } else { "SHORT" }, name.to_uppercase()),
                            &sizing.map_or_else(|| format!("{name} {leverage}"), |s| format!("{} · {leverage}", Fmt::usd(s.notional))), lines, None, &state(position.status, false)));
        }
        if let Some(protection) = block("protection") {
            let mut lines = protection.lines.clone();
            if let (Some(z), Some(stop)) = (sizing, r.stop_pct) {
                lines = vec![row(&say("stop", "стоп"), &format!("{} · −{}", Fmt::price(z.stop_price), Fmt::pct(stop)))];
                if let Some(tp) = z.take_price {
                    let moved = pct(((Num::of(tp) - z.entry).abs() / z.entry * 100).rounded(1, Rounding::Plain));
                    lines.push(row(&say("take", "тейк"), &format!("{} · +{moved}", Fmt::price(tp))));
                }
                lines.push(say("both wait on the exchange", "оба ждут на бирже"));
            }
            steps.push(step("PROTECT", &if sizing.is_some_and(|s| s.take_price.is_some()) { say("Stop + take", "Стоп + тейк") } else { say("Stop", "Стоп") },
                            lines, None, &state(protection.status, false)));
        }
        // What Confirm signs: the simulation ran first, then Windows Hello for every signature.
        let signing = p.map_or_else(|| blocks.iter().filter(|b| ["bridge", "position", "protection"].contains(&b.id.as_str()) && b.status != BlockStatus::Info).count(),
                                    |p| p.steps.len());
        if sizing.is_some() && signing > 0 {
            if let Some(p) = p {
                steps.push(step("SIMULATION · PASSED", &say("As planned", "Как задумано"), p.changes.iter().take(3).cloned().collect(), None, "passed"));
            } else {
                steps.push(step("SIMULATION · FIRST", &say("Runs before the card", "До карточки"),
                                vec![say("every balance, before and after", "каждый баланс — до и после"), say("nothing is signed before it", "до неё ничего не подписывается")], None, "info"));
            }
            steps.push(step(if ledger { "SIGN · LEDGER" } else { "SIGN · WINDOWS HELLO" },
                            &say(&format!("{signing} signature{}", if signing == 1 { "" } else { "s" }), &format!("подписей: {signing}")),
                            p.map_or_else(|| vec![if ledger { say("each one approved on the device", "каждая — на устройстве") } else { say("one Windows Hello covers them all", "одно Windows Hello на все") }],
                                          |p| p.steps.clone()),
                            None, "info"));
        }
        if let Some(s) = sizing {
            let loss = Fmt::usd(s.loss_at_stop_usd);
            let mut lines = vec![row(&say("at the stop", "на стопе"), &format!("−{loss}"))];
            if let Some(profit) = s.profit_at_take_usd { lines.push(row(&say("at the take", "на тейке"), &format!("+{}", Fmt::usd(profit)))) }
            lines.push(row(&say("fees", "комиссии"), &format!("≈ {} · ihatecrypto {}", Fmt::usd(s.fees_usd), Fmt::usd(s.builder_fees_usd))));
            steps.push(step("RESULT · ESTIMATE", &format!("−{loss}{}", s.profit_at_take_usd.map(|p| format!(" / +{}", Fmt::usd(p))).unwrap_or_default()), lines, None, "info"));
        }
        let (signer, signer_ru) = if ledger { ("your Ledger", "Ledger") } else { ("Windows Hello", "Windows Hello") };
        let footer = if watching {
            say("Watching only: the simulation shows what would happen. Nothing can sign here.", "Только просмотр: симуляция показывает, что было бы. Подписать здесь нечем.")
        } else if p.is_some() {
            say(&format!("Confirm signs {signing} signature{} after {signer}. The simulation above ran first.", if signing == 1 { "" } else { "s" }),
                &format!("«Подтвердить» подпишет {signing} — после {signer_ru}. Симуляция выше уже прошла."))
        } else {
            say(&format!("Nothing is signed until the plan passes its checks: the simulation runs first, then {signer}."),
                &format!("Ничего не подписывается, пока план не пройдёт проверки: сначала симуляция, потом {signer_ru}."))
        };
        (steps, footer)
    }

    // MARK: Planning

    /// A trade plan by the risk rules (Moves.plan forwards here): its route drawn whether the rules stop it or not, the
    /// chat's checklist, and — once it passes — the card with every signature and the simulation, in the plan's place.
    pub async fn plan_trade(&self, next: TradeIntent, note: Option<&str>, said: &str) {
        let (before, traded) = { let st = self.st(); (st.intent.clone(), st.traded_plan.clone()) };
        // After a trade «stop 2%» or «take closer» move that position's orders instead of starting a new plan.
        if let Some(traded) = traded && self.edit_traded(&traded, &next, note).await { return }
        self.st_mut().intent = next.clone();
        self.plan_markets_loaded().await;
        let mut market = self.st().markets.get(&next.asset).cloned();
        let mut atr_read = true;
        if let Some(m) = market.as_mut() && m.atr_pct.is_none() {
            let name = m.name.clone();
            m.atr_pct = self.plan_atr(&name).await;
            atr_read = m.atr_pct.is_some();
        }
        let market = market.as_ref();
        let (equity, policy, russian) = (self.rule_equity(), self.policy(), self.st().russian());
        let (r, notes) = Plan::resolve(&next, market, equity, &policy, None);
        let e = Plan::evaluate(&r, market);
        let quoted = self.plan_quote(&r, &e, market).await;
        let blocks = Plan::blocks(&r, &e, &self.plan_live(market, notes, quoted.as_ref()));
        let changes = next.changes(&before);
        let lead = (if said.is_empty() { String::new() } else { format!("{said}\n") })
            + &if changes.is_empty() { self.say("That’s already in the plan.", "Это уже есть в плане.") } else {
                self.say("Plan updated: ", "План обновлён: ")
                    + &changes.iter().map(|c| if russian { Self::change_ru(c) } else { c.clone() }).collect::<Vec<_>>().join(", ") + "."
            };
        let pretty = IntentParser::pretty(&r.asset);
        let title = if r.side == Side::Long { self.say(&format!("Plan · long {pretty}"), &format!("План · лонг {pretty}")) }
            else { self.say(&format!("Plan · short {pretty}"), &format!("План · шорт {pretty}")) };
        let mut view = PlanView {
            market: r.asset.clone(), group: market.map_or(MarketGroup::Crypto, Market::group).raw().into(), stopped: None, short: None, blocked: false,
            fix: None, fix_detail: None, over_limit: false, takes: None, footer: String::new(), preview: false,
        };

        // Stopped, or waiting for a detail: the route with what stops it, the fix, the checklist.
        // A bridge counts for Modules whenever the plan would take one: a sum named, or a quote for the money it moves (the
        // Mac's tradeRefusal: amountUSDC != nil || quote != nil).
        let off = trade::modules().refusal_trade(market.is_some_and(|m| m.dex.is_some()), next.amount_usdc.is_some() || quoted.is_some(), russian);
        let from = quoted.as_ref().and_then(|q| q.from.as_ref()).map_or("Solana", |n| n.name);
        let blocker = match &off { Some(o) => Some((self.say("off in Modules", "выключено в модулях"), o.clone())), None => self.plan_blocker(&e, &blocks, equity, from) };
        // Ready by the rules, a TWAP still needs the trading key on (the Mac's twapRefusal at Execute): no card without it.
        let twap = if blocker.is_none() && e.missing.is_empty() && market.is_some() { self.twap_refusal(&next) } else { None };
        let fixable = off.is_none() && twap.is_none();
        let blocker = blocker.or_else(|| twap.map(|t| (self.say("needs the trading key", "нужен торговый ключ"), t)));
        if blocker.is_some() || !e.missing.is_empty() || market.is_none() {
            let (steps, footer) = self.trade_route(&r, &e, &blocks, market, None, None, false, None, quoted.as_ref());
            view.footer = footer;
            if let Some((short, sentence)) = blocker {
                view.stopped = Some(sentence);
                view.short = Some(short);
                view.blocked = true;
                if fixable && let Some(m) = market && let Some((title, detail, command)) = self.plan_fix(&next, m, e.sizing.as_ref()) {
                    view.fix = Some(Action { title, command });
                    view.fix_detail = Some(detail);
                }
                view.over_limit = e.sizing.as_ref().is_some_and(|s| equity.is_some_and(|eq| eq > Decimal::ZERO
                    && (Num::of(s.loss_at_stop_usd) / eq * 100).0.is_some_and(|x| x > policy.max_risk_pct)));
            } else if !e.missing.is_empty() {
                let missing = e.missing.iter().map(|m| if m == "risk in $" { self.say("the risk", "риска") } else { self.say("a stop", "стопа") })
                    .collect::<Vec<_>>().join(&self.say(" and ", " и "));
                view.short = Some(self.say(&format!("needs {missing}"), &format!("нет {missing}")));
                view.stopped = Some(self.say(&format!("The plan needs {missing} before it can be checked."), &format!("Плану не хватает {missing}, чтобы его проверить.")));
                if next.auto_rules == Some(true) {
                    // The book is on and still can't fill it: no capital to take the risk from, or no candles for the stop.
                    let (en, ru) = Self::missing_example(&e.missing);
                    view.stopped = Some(self.say(&format!("The plan needs {missing}: the rules have nothing to size it from yet — say it, e.g. “{en}”."),
                                                 &format!("Плану не хватает {missing}: правилам пока не от чего его посчитать — скажи сам, например «{ru}».")));
                } else {
                    let (rr, risk) = (Fmt::num(policy.default_reward_risk, 2), Fmt::pct(policy.default_risk_pct));
                    view.fix = Some(Action { title: self.say("By the book", "По правилам"), command: "by the book".into() });
                    view.fix_detail = Some(self.say(&format!("Stop from the 4h volatility, take at {rr}R, {risk} of capital at risk."),
                                                    &format!("Стоп по волатильности за 4 часа, тейк {rr}R, риск {risk} капитала.")));
                }
            }
            self.draw_plan(&title, steps, view, &r, &e, market);
            let list = self.plan_checklist(&next, &r, &e, &blocks, market, atr_read, false, quoted.as_ref(), None);
            self.reply(&format!("{lead}\n{list}"), note.map(String::from));
            if said.is_empty() { self.comment_on_plan(&list, note) }
            return;
        }

        // Code first, second check on or off: the plan never holds more money or more risk than its command named.
        let mut warnings: Vec<String> = Vec::new();
        if let (Some(m), Some(s)) = (market, &e.sizing) && let Some(why) = Plan::money_problem(&next, s, m) { warnings.push(why) }
        let watching = self.st().watching();
        // A new Hyperliquid account opens with this trade: Relay or NEAR Intents, picked on two cards first (move_checks.rs).
        if !watching && self.ask_opening_route(&r, &e, market, note).await { return }
        let one_click = self.st().open_via_1click == Some(true);
        let executor = if watching { self.preview_executor() } else { self.executor() };
        let prepared = match executor {
            // ExecutionError in Swift: «Add a wallet first», the watched addresses' refusal.
            Err(e) => Err(trade::Error::Execution(e.0)),
            Ok(x) => x.prepare(&next, &policy, one_click, &HashMap::new()).await,
        };
        match prepared {
            Ok(p) => {
                // A code mismatch is the check's doubt, second check on or off (the Mac's planReview «mismatch»); otherwise the
                // second check reads it, and one that couldn't run says so on the route.
                let mut review = warnings.first().cloned();
                let mut not_checked: Option<String> = None;
                if warnings.is_empty() && self.second_check() {
                    match self.review_plan(&next, &p).await {
                        PlanReview::Doubt(concern) => { review = Some(concern.clone()); warnings.push(concern) }
                        PlanReview::NotChecked(why) => not_checked = Some(why),
                        PlanReview::Matches => {}
                    }
                }
                let reviewed = self.second_check() || review.is_some();
                let (steps, footer) = self.trade_route(&r, &e, &blocks, market, Some(&p), review.as_deref(), reviewed, not_checked.as_deref(), None);
                view.footer = footer;
                if watching {
                    // A watched address: the simulation on the canvas, nothing to confirm.
                    view.preview = true;
                    self.draw_plan(&title, steps, view, &r, &e, market);
                } else {
                    let replaced = self.st().plan_board.clone();
                    // A broken limit in «Just warn» is on the card too, not only in the chat («Ask me» has its tick).
                    let mut card_warnings = warnings.clone();
                    if !p.must_accept { card_warnings.extend(if self.st().russian() { p.over_limit_ru.clone() } else { p.over_limit.clone() }) }
                    self.show(Pending::Trade(Box::new(p.clone())), &title, [p.steps.clone(), p.changes.clone()].concat(), card_warnings, vec![]);
                    // Over a risk limit with «Ask me», the second check's doubt, or a money mismatch code found: ticked before
                    // Confirm counts (the Mac's Execute sheet).
                    let mut ticks = vec![];
                    if p.must_accept {
                        let detail = if self.st().russian() { &p.over_limit_ru } else { &p.over_limit };
                        ticks.push(Self::over_limit_tick(&detail.join(" "), p.sizing.loss_at_stop_usd, |en, ru| self.say(en, ru)));
                    }
                    if let Some(review) = &review { ticks.push(Self::trade_tick(review, |en, ru| self.say(en, ru))) }
                    if let Some(card) = self.st_mut().card.as_mut() {
                        card.over_limit = !p.over_limit.is_empty();
                        if !ticks.is_empty() { card.ticks = ticks }
                    }
                    let card = self.st().card.as_ref().map(|c| c.id.clone());
                    self.update_board(card.as_deref(), |b| { b.steps = Some(steps); b.plan = Some(view) });
                    self.draw_trade(card.as_deref(), &r, e.sizing.as_ref(), market);
                    // The stopped plan, or the card it was before an edit: the new card takes its place.
                    if let Some(replaced) = replaced && Some(&replaced) != card.as_ref() {
                        self.set_boards(|bs| bs.retain(|b| !(b.id == replaced && PLAN_STATES.contains(&b.state.as_deref().unwrap_or("")))));
                    }
                    self.st_mut().plan_board = card;
                }
                let list = self.plan_checklist(&next, &r, &e, &blocks, market, atr_read, !watching, quoted.as_ref(), None);
                self.reply(&format!("{lead}\n{list}"), note.map(String::from));
                if said.is_empty() { self.comment_on_plan(&list, note) }
            }
            Err(error) => {
                // The preparation refused it (a balance moved, the bridge couldn't quote): the route, stopped by that.
                let why = error.to_string();
                let (steps, footer) = self.trade_route(&r, &e, &blocks, market, None, None, false, None, quoted.as_ref());
                view.footer = footer;
                view.stopped = Some(why.clone());
                view.short = Some(self.say("not prepared", "не подготовлен"));
                view.blocked = true;
                self.draw_plan(&title, steps, view, &r, &e, market);
                let list = self.plan_checklist(&next, &r, &e, &blocks, market, atr_read, false, quoted.as_ref(), Some(&why));
                self.reply(&format!("{lead}\n{list}"), note.map(String::from));
                // The model works out why and the way that works (the Mac's Execution.swift:334 noteFailure).
                self.note_refusal_error(&title, &error, note);
            }
        }
    }

    /// The plan's board, stopped or previewed: drawn in the place of the one before it, the newest on top.
    fn draw_plan(&self, title: &str, steps: Vec<Step>, view: PlanView, r: &TradeIntent, e: &Evaluation, market: Option<&Market>) {
        // A card from an earlier version of this plan stops waiting: what's on the canvas now is this one.
        let (stale, plan_board) = {
            let st = self.st();
            (matches!(st.pending, Some(Pending::Trade(_))) && st.card.as_ref().map(|c| c.id.clone()) == st.plan_board, st.plan_board.clone())
        };
        if stale { self.cancel_pending(None) }
        if let Some(id) = plan_board && self.st().boards().iter().any(|b| b.id == id && PLAN_STATES.contains(&b.state.as_deref().unwrap_or(""))) {
            self.set_boards(|bs| bs.retain(|b| b.id != id));
        }
        let id = self.pin(Board { steps: Some(steps), state: Some("plan".into()), ..Board::new("move", title) });
        self.st_mut().plan_board = Some(id.clone());
        self.update_board(Some(&id), |b| b.plan = Some(view));
        self.draw_trade(Some(&id), r, e.sizing.as_ref(), market);
    }

    /// The plan's board goes with the plan («clear the plan», its Cancel), and «stop 2%» no longer edits the last trade.
    pub fn drop_plan_board(&self) {
        self.st_mut().traded_plan = None;
        let Some(id) = self.st().plan_board.clone() else { return };
        if self.st().card.as_ref().is_some_and(|c| c.id == id) { self.cancel_pending(None) }
        self.set_boards(|bs| bs.retain(|b| !(b.id == id && PLAN_STATES.contains(&b.state.as_deref().unwrap_or("")))));
        self.st_mut().plan_board = None;
    }

    /// The model's own word on the plan it built (ChatRouter.comment, the Mac's replyPlan): it takes the first line's
    /// place, with the changes worth making as cards. Dropped when anything was said or changed since.
    pub fn comment_on_plan(&self, list: &str, _note: Option<&str>) {
        let Some(client) = self.llm_client(None) else { return };
        let (sent, plan, command, user_text) = {
            let st = self.st();
            let Some(sent) = st.messages().last().filter(|m| !m.from_user).map(|m| m.id.clone()) else { return };
            let command = st.intent.command();
            let user_text = st.messages().iter().rev().find(|m| m.from_user).map_or_else(|| command.clone(), |m| m.text.clone());
            (sent, st.intent.clone(), command, user_text)
        };
        let (context, model, list, me) = (self.router_context(), self.model(), list.to_string(), self.rc());
        tokio::task::spawn_local(async move {
            let Ok(reading) = ChatRouter::comment(&list, &command, &user_text, &context, &client, &model).await else { return };
            let Route::Chat(words) = &reading.route else { return };
            if words.is_empty() { return }
            {
                let mut st = me.st_mut();
                if st.intent != plan || st.messages().last().map(|m| m.id.as_str()) != Some(sent.as_str()) { return }
                if let Some(m) = st.messages_mut().last_mut() { m.text = format!("{words}\n{list}") }
            }
            me.save_chat();
            me.changed();
            let options = reading.options.iter().map(|c| RouterOption::new(&c.title, &c.comment, &c.command, c.recommended)).collect();
            me.offer(options, &user_text, reading.found.clone());
        });
    }

    /// Between the balance reads, while the guardian has rules: the positions and orders read again and its pass run.
    pub async fn guard_now(&self) {
        if self.exit_rules().is_empty() { return }
        let Some(me) = self.st().evm_address().filter(|a| !a.is_empty()) else { return };
        let (before, same_wallet) = {
            let st = self.st();
            (st.positions.iter().map(|p| p.coin.clone()).collect::<Vec<_>>(), st.portfolio_owner == st.read_wallet().map(|w| format!("{}{}", w.solana, w.evm)))
        };
        let (now, orders) = tokio::join!(HyperliquidAccount::positions(&me), HyperliquidAccount::open_orders(&me));
        // Another wallet since the read began: they aren't its positions (the Mac's user == hlAddress).
        if self.st().evm_address().as_deref() != Some(me.as_str()) { return }
        {
            let mut st = self.st_mut();
            if let Ok(now) = now {
                st.positions = now.into_iter().filter(|p| !p.size.is_zero()).collect();
                st.positions_for = Some(me.to_lowercase());
            }
            if let Ok(orders) = orders { st.hl_orders = orders }
        }
        // One its stop closed between the reads is said here, as refresh() would.
        if same_wallet { self.note_closed_positions(&before).await }
        self.guard_positions().await;
    }

    /// A plan for watched addresses: prepared and simulated against them, never signed (the Mac's previewWallet).
    pub fn preview_executor(&self) -> Result<Arc<Executor>, WalletError> {
        let (solana, evm, refusal) = { let st = self.st(); (st.settings.watch_solana.clone(), st.settings.watch_evm.clone(), st.watch_only()) };
        let (Some(solana), Some(evm)) = (solana, evm) else {
            return Err(WalletError(self.say("A preview needs both watched addresses, Solana and EVM: the money comes from one and trades on the other.",
                                            "Для просмотра нужны оба адреса — Solana и EVM: деньги идут с одного, сделка — на другом.")));
        };
        Ok(Executor::new(TradeWallet { solana, evm }, Arc::new(PreviewSigner { refusal }), self.journal(), |_| {}))
    }

    /// The plan the next words change: the one on the canvas, or the traded one while its position is open.
    pub fn plan_base(&self) -> TradeIntent {
        let st = self.st();
        if st.intent == TradeIntent::default() && let Some(traded) = &st.traded_plan && st.positions.iter().any(|p| p.coin == traded.asset) {
            return traded.clone();
        }
        st.intent.clone()
    }

    // MARK: After a trade

    /// «stop 2%», «take closer» after a trade: only stop and take may change, and that position's orders move on the
    /// exchange after Confirm (the Mac's editExecuted); anything else is a new plan.
    pub async fn edit_traded(&self, before: &TradeIntent, next: &TradeIntent, note: Option<&str>) -> bool {
        let p = self.st().positions.iter().find(|p| p.coin == before.asset).cloned();
        let Some(p) = p else { self.st_mut().traded_plan = None; return false };
        let mut same = next.clone();
        same.stop_pct = before.stop_pct;
        same.take_pct = before.take_pct;
        same.take_r = before.take_r;
        if same != *before || next == before { return false }
        let mut market = self.st().markets.get(&before.asset).cloned();
        if let Some(m) = market.as_mut() && m.atr_pct.is_none() {
            let name = m.name.clone();
            m.atr_pct = self.plan_atr(&name).await;
        }
        let (equity, policy) = (self.rule_equity(), self.policy());
        let resolved = Plan::resolve(next, market.as_ref(), equity, &policy, None).0;
        let (mut moves, too_wide) = Self::order_edit(before, next, &resolved, p.size, p.entry, equity, policy.max_risk_pct);
        let mut over = None;
        if let Some(loss) = too_wide && let Some(equity) = equity && let Some(stop_pct) = resolved.stop_pct {
            let (share, stop, lost, max) = (pct((Num::of(loss) / equity * 100).rounded(1, Rounding::Plain)), Fmt::pct(stop_pct), Fmt::usd(loss), Fmt::pct(policy.max_risk_pct));
            if policy.limit_mode() == OverLimit::Block {
                self.reply(&self.say(&format!("A {stop} stop means −{lost}, {share} of capital. The rule: at most {max}. Stop not moved."),
                                     &format!("Стоп {stop} — это −{lost}, {share} капитала. Правило: не больше {max}. Стоп не сдвинут.")), note.map(String::from));
                return true;
            }
            // «Ask me» and «Just warn»: the card says the loss, and Confirm moves it.
            moves = Self::order_edit(before, next, &resolved, p.size, p.entry, None, RiskPolicy::NO_LIMIT).0;
            over = Some(self.say(&format!(" Over your limit: a {stop} stop means −{lost}, {share} of capital, the limit is {max}."),
                                 &format!(" Выше твоего лимита: стоп {stop} — это −{lost}, {share} капитала, лимит {max}.")));
        }
        if moves.is_empty() { return false }
        let what = moves.iter().map(|m| self.describe_order(m)).collect::<Vec<_>>().join(", ");
        let name = IntentParser::pretty(&p.coin);
        let warnings = over.iter().map(|o| o.trim().to_string()).collect();
        self.show(Pending::Orders { commands: moves, coin: p.coin.clone(), intent: next.clone() }, &format!("{name}: {what}"), vec![], warnings, vec![]);
        let over = over.unwrap_or_default();
        self.reply(&self.say(&format!("{name}: {what}.{over} Press Confirm to move the orders."), &format!("{name}: {what}.{over} Нажми «Подтвердить», чтобы переставить ордера.")),
                   note.map(String::from));
        true
    }

    /// Confirm on «stop 2%» after a trade: each order moved on the exchange, the traded plan now with the new levels. The
    /// executor's own error, so the caller tells a guard's stop from a failure (MoveChecks.explainsAfter).
    pub async fn move_orders(&self, moves: &[PositionCommand], coin: &str, traded: &TradeIntent) -> Result<String, trade::Error> {
        let mut said = Vec::new();
        for m in moves {
            let x = self.executor().map_err(|e| trade::Error::Execution(e.0))?;
            let (policy, equity) = (self.policy(), self.rule_equity());
            said.push(x.manage(m, coin, &policy, equity).await?);
        }
        self.st_mut().traded_plan = Some(traded.clone());
        Ok(said.join("\n"))
    }

    /// The orders «stop 2%», «take 6%» move on an open position: only what the words changed, at the new levels from its
    /// entry. The second value: what the new stop would lose when that's over the per-trade limit; nothing moves then.
    pub fn order_edit(before: &TradeIntent, asked: &TradeIntent, resolved: &TradeIntent, size: Decimal, entry: Decimal, equity: Option<Decimal>,
                      max_risk_pct: Decimal) -> (Vec<PositionCommand>, Option<Decimal>) {
        let dir: i64 = if size > Decimal::ZERO { 1 } else { -1 };
        let mut moves = Vec::new();
        if asked.stop_pct != before.stop_pct && let Some(stop_pct) = resolved.stop_pct {
            let stop = Num::of(entry) * (1 - dir * Num::of(stop_pct) / 100);
            let loss = (Num::of(entry) - stop).abs() * size.abs() + Num::of(size.abs()) * (Num::of(entry) + stop) * RiskCalc::trade_fee();
            if let Some(equity) = equity && equity > Decimal::ZERO && (loss / equity * 100).0.is_some_and(|x| x > max_risk_pct) { return (vec![], loss.0) }
            // ponytail: a stop past the arithmetic's range isn't moved, where Swift would have sent NaN on.
            if let Some(stop) = stop.0 { moves.push(PositionCommand::StopTo(stop)) }
        }
        if (asked.take_pct != before.take_pct || asked.take_r != before.take_r) && let Some(take_pct) = resolved.take_pct
            && let Some(take) = (Num::of(entry) * (1 + dir * Num::of(take_pct) / 100)).0 {
            moves.push(PositionCommand::TakeTo(take));
        }
        (moves, None)
    }

    pub fn describe_order(&self, command: &PositionCommand) -> String {
        match command {
            PositionCommand::Close(share) => if *share >= Decimal::ONE { self.say("close", "закрыть") } else {
                let p = (Num::of(*share) * 100).0.map_or_else(|| "NaN".into(), |x| Fmt::num(x, 0));
                self.say(&format!("close {p}%"), &format!("закрыть {p}%"))
            },
            PositionCommand::StopToBreakeven => self.say("stop to breakeven", "стоп в безубыток"),
            PositionCommand::StopTo(px) => self.say(&format!("stop to {}", Fmt::price(*px)), &format!("стоп на {}", Fmt::price(*px))),
            PositionCommand::TakeTo(px) => self.say(&format!("take-profit to {}", Fmt::price(*px)), &format!("тейк на {}", Fmt::price(*px))),
            PositionCommand::Trail(pct) => self.say(&format!("trail the stop {}", Fmt::pct(*pct)), &format!("трейлинг-стоп {}", Fmt::pct(*pct))),
            PositionCommand::CancelOrders => self.say("cancel orders", "снять ордера"),
        }
    }

    // MARK: Take profit, the market

    /// «Take profit like the pros»: the three ways, on the plan's board; true when the message asked for them.
    pub async fn ask_take_choices(&self, text: &str, note: Option<&str>) -> bool {
        if !TakeStyle::asks_for_choice(text) { return false }
        let (intent, market, board) = { let st = self.st(); (st.intent.clone(), st.markets.get(&st.intent.asset).cloned(), st.plan_board.clone()) };
        if intent == TradeIntent::default() {
            self.reply(&self.say("Describe the trade first — then I’ll show the ways to take profit on it.", "Сначала опиши сделку — потом покажу, как на ней взять прибыль."),
                       note.map(String::from));
            return true;
        }
        let resolved = Plan::resolve(&intent, market.as_ref(), self.rule_equity(), &self.policy(), None).0;
        let risk = Plan::evaluate(&resolved, market.as_ref()).sizing.map(|s| s.loss_at_stop_usd);
        let takes = self.take_choices(risk);
        self.update_board(board.as_deref(), |b| if let Some(plan) = b.plan.as_mut() { plan.takes = Some(takes) });
        self.st_mut().takes_at = Some(Utc::now());
        self.reply(&self.say("Three ways to take profit are on the canvas. The stop stays the same. Pick one or say your own, e.g. “take 3R”.",
                             "На холсте три способа взять прибыль. Стоп остаётся прежним. Выбери один или скажи свой, например «тейк 3R»."), note.map(String::from));
        true
    }

    pub fn take_choices(&self, risk: Option<Decimal>) -> Vec<TakeChoice> {
        TakeStyle::ALL.into_iter().map(|s| {
            let ru = match s {
                TakeStyle::Classic => ("Классика 2R", "Всё на двойном риске.", "В плюсе, если выигрываешь больше трети сделок."),
                TakeStyle::Partial => ("По частям", "Половина на 1R, стоп в безубыток, остальное на 2R.", "После первой половины риск почти ноль."),
                TakeStyle::Trend => ("За трендом", "Половина на 1R, остальное идёт за ценой с трейлинг-стопом 2%.", "Без потолка, но на развороте часть отдашь."),
            };
            // A risk too big to double (a number from the chat) has no payoff to say rather than taking the engine down.
            let payoff = if s == TakeStyle::Trend { self.say("no cap", "без потолка") }
                else { risk.filter(|r| r.checked_mul(Decimal::TWO).is_some()).map(|r| s.payoff(r)).unwrap_or_default() };
            TakeChoice { id: s.raw().into(), title: self.say(s.title(), ru.0), detail: self.say(s.detail(), ru.1), note: self.say(s.note(), ru.2), payoff,
                         recommended: s == TakeStyle::Partial }
        }).collect()
    }

    /// A take-profit way picked on the board: the plan takes it, and is checked again.
    pub async fn choose_take(&self, id: &str) {
        let Some(style) = TakeStyle::ALL.into_iter().find(|s| s.raw() == id) else { return };
        let intent = self.st().intent.clone();
        if intent == TradeIntent::default() { return }
        let ru = self.take_choices(None).into_iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.title.to_lowercase());
        let said = self.say(&format!("Take: {}.", style.title().to_lowercase()), &format!("Тейк: {ru}."));
        self.plan_trade(style.apply(&intent), None, &said).await;
    }

    /// The market picked in the plan's header (the Mac's MarketPicker): the same plan on that market, checked again.
    pub async fn choose_plan_market(&self, name: &str) {
        let (intent, m) = { let st = self.st(); (st.intent.clone(), st.markets.get(name).cloned()) };
        if intent == TradeIntent::default() || name == intent.asset { return }
        let Some(m) = m else { return };
        let mut next = intent;
        next.asset = m.name.clone();
        let (pretty, dex) = (IntentParser::pretty(&m.name), m.dex.as_ref().map(|d| format!(" {d}")).unwrap_or_default());
        let said = self.say(&format!("Market: {pretty} · Hyperliquid{dex} — {}.", m.group().raw().to_lowercase()),
                            &format!("Рынок: {pretty} · Hyperliquid{dex} — {}.", group_ru(m.group())));
        self.plan_trade(next, None, &said).await;
    }

    /// What to say for what the plan lacks: «risk $1», «stop 3%» or both.
    pub fn missing_example(missing: &[String]) -> (&'static str, &'static str) {
        match missing {
            [m] if m == "risk in $" => ("risk $1", "риск $1"),
            [m] if m == "stop in %" => ("stop 3%", "стоп 3%"),
            _ => ("risk $1 stop 3%", "риск $1 стоп 3%"),
        }
    }

    /// TradeIntent.changes in Russian: its fragments are the core's English.
    pub fn change_ru(change: &str) -> String {
        const WHOLE: [(&str, &str); 5] = [
            ("long", "лонг"), ("short", "шорт"), ("the rest by the risk rules", "остальное по правилам риска"), ("market entry", "вход по рынку"),
            ("take in parts: half at 1R, then stop to breakeven", "тейк по частям: половина на 1R, потом стоп в безубыток"),
        ];
        const STARTS: [(&str, &str); 9] = [
            ("market ", "рынок "), ("risk ", "риск "), ("stop ", "стоп "), ("take ", "тейк "), ("leverage ", "плечо "), ("money in the trade ", "деньги в сделке "),
            ("trailing ", "трейлинг "), ("limit entry at ", "лимитный вход на "), ("entry on a ", "вход на откате "),
        ];
        if let Some((_, ru)) = WHOLE.iter().find(|(en, _)| *en == change) { return ru.to_string() }
        // «TWAP over 2 h, randomized», «one market order»: a TWAP's entry or its end.
        if change == "one market order" { return "вход одним рыночным ордером".into() }
        if let Some(rest) = change.strip_prefix("TWAP over ") {
            let (time, random) = rest.strip_suffix(", randomized").map_or((rest, ""), |t| (t, ", части случайного размера"));
            return format!("TWAP за {}{random}", Self::time_ru(time));
        }
        for (en, ru) in STARTS {
            if let Some(rest) = change.strip_prefix(en) { return format!("{ru}{}", rest.replace(" pullback", "")) }
        }
        change.into()
    }

    /// TradeIntent::duration in Russian: «2 ч», «90 мин», «3 дн».
    pub fn time_ru(time: &str) -> String { time.replace(" min", " мин").replace(" h", " ч").replace(" d", " дн") }

    /// A TWAP keeps buying for hours: only a trading key that's on can stop it with no one at the PC (its stop fired, the
    /// loss grew). Without one it isn't started (the Mac's twapRefusal): the plan says so, and Confirm refuses it too.
    pub fn twap_refusal(&self, intent: &TradeIntent) -> Option<String> {
        if intent.twap_minutes.is_none() || self.agent_active() { return None }
        Some(if self.ledger_active() {
            self.say("A TWAP needs the Ledger's trading key on: without it, stopping the TWAP waits for the device's buttons. Say «approve trading key», or buy in one order.",
                     "Для TWAP нужен торговый ключ Ledger: без него остановка TWAP ждёт кнопок устройства. Скажи «approve trading key» или купи одним ордером.")
        } else {
            self.say("A TWAP needs the Hyperliquid trading key on (Settings › Wallet › Hyperliquid): locked or off, nothing can stop the TWAP while you're away. Turn it on, or buy in one order.",
                     "Для TWAP нужен включённый торговый ключ Hyperliquid (Settings › Wallet › Hyperliquid): когда он выключен или заблокирован, TWAP некому остановить без тебя. Включи его или купи одним ордером.")
        })
    }

    /// The window's requests about the plan: the picker's markets (read only), a market or a take-profit way picked, Cancel.
    pub async fn plan_op(&self, r: &Request) -> HashMap<String, String> {
        let name = r.name.clone().unwrap_or_default();
        match r.op.as_str() {
            "planMarkets" => return HashMap::from([("json".to_string(), self.plan_markets().await)]),
            "planMarket" => self.choose_plan_market(&name).await,
            "planTake" => self.choose_take(&name).await,
            _ => {
                self.clear_plan(None);
                self.drop_plan_board();
            }
        }
        HashMap::new()
    }

    /// Hyperliquid's markets by group, sorted by name as the Mac's picker: {"Crypto": ["BTC", …], "Stocks": […]}.
    pub async fn plan_markets(&self) -> String {
        self.plan_markets_loaded().await;
        let st = self.st();
        let out: BTreeMap<&str, Vec<String>> = MarketGroup::ALL.into_iter().map(|g| {
            let mut names: Vec<String> = st.markets.values().filter(|m| m.group() == g).map(|m| m.name.clone()).collect();
            names.sort();
            (g.raw(), names)
        }).collect();
        serde_json::to_string(&out).unwrap_or_else(|_| "{}".into())
    }

    // MARK: The position board

    pub fn position_ruler(&self, p: &Position, rules: Option<&ExitRules>) -> PositionRuler {
        let (triggers, mark) = {
            let st = self.st();
            (st.hl_orders.iter().filter(|o| o.coin == p.coin && o.is_trigger).cloned().collect::<Vec<_>>(), st.markets.get(&p.coin).map(|m| m.mark_px))
        };
        let stop = triggers.iter().find(|o| kind(o).contains("Stop")).and_then(|o| o.trigger_px);
        let takes = triggers.iter().filter(|o| kind(o).contains("Take")).filter_map(|o| o.trigger_px);
        let point = |kind: &str, at: Decimal, en: &str, ru: &str| RulerPoint { kind: kind.into(), at: double(at), text: self.say(en, ru) + &Fmt::price(at) };
        let mut points = vec![point("entry", p.entry, "entry ", "вход ")];
        if let Some(stop) = stop { points.push(point("stop", stop, "stop ", "стоп ")) }
        points.extend(takes.map(|t| point("take", t, "take ", "тейк ")));
        let guard_line = rules.map(|r| if r.moved_to_breakeven { self.say("Guardian: stop already at breakeven", "Страж: стоп уже в безубытке") }
            else if r.breakeven_after_partial { self.say("Guardian: stop to breakeven after the first half", "Страж: стоп в безубыток после первой половины") }
            else { self.say("Guardian is watching", "Страж следит") });
        PositionRuler { points, now: mark.map(double), now_text: mark.map(Fmt::price), guard_line }
    }
}

#[cfg(test)]
#[path = "trade_plan/tests.rs"]
mod tests;

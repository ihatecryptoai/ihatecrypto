//! `Intent.swift`: the trade a chat asks for (`TradeIntent`), the deterministic parser of a trader's phrases
//! (`IntentParser`), the guard that keeps a recovery phrase or a private key from the model (`SeedGuard`), and `Fmt`'s
//! money, amount, price and date texts.

use std::collections::HashSet;

use chrono::Local;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::foundation::{
    Date, Rounding, all, all_chars, count, decimal, first, group, has, is_letter, is_number, number, replacing, rounded, split_chars, text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Long,
    Short,
}

/// What the user wants. Only words and numbers from the chat; never addresses or keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeIntent {
    pub asset: String,
    pub side: Side,
    #[serde(rename = "riskUSD", default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub risk_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub stop_pct: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub take_pct: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub leverage: Option<Decimal>,
    /// USDC to send from Solana. nil = exactly what the position needs.
    #[serde(rename = "amountUSDC", default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub amount_usdc: Option<Decimal>,
    /// Take as a multiple of the stop distance («тейк 2R», «1:3»); used when takePct is not set.
    #[serde(rename = "takeR", default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub take_r: Option<Decimal>,
    /// «как учат»: what the user left out is filled by the risk rules (stop from volatility, 2R take, 1% risk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_rules: Option<bool>,
    /// Limit entry price («лимит 2650»); nil = market entry.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub entry_px: Option<Decimal>,
    /// Limit entry this far from the mark in the trade's favour («вход на откате 2%»); resolved to entryPx.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub pullback_pct: Option<Decimal>,
    /// «тейк частями»: half at 1R, then the stop goes to breakeven and the rest runs to the take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_take: Option<bool>,
    /// «трейлинг 2%»: the stop follows the best price at this distance while the app is open.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub trailing_pct: Option<Decimal>,
    /// «за 2 часа», «over 30 min»: the entry is one TWAP on Hyperliquid over this many minutes (5 min to 4 h here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twap_minutes: Option<i64>,
    /// «randomize»: each part of the TWAP up to ±20% of its size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twap_randomize: Option<bool>,
    /// «hedge 0.1 SOL»: a short of this many of the coins the wallet holds (Hedge); sized by them, not by a risk.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub hedge: Option<Decimal>,
    /// «hedge SOL» = 1, «hedge SOL half» = 0.5: the share of what the wallet holds, turned into `hedge` by Plan.resolve.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub hedge_share: Option<Decimal>,
}

impl Default for TradeIntent {
    fn default() -> Self {
        Self {
            asset: "ETH".into(), side: Side::Long, risk_usd: None, stop_pct: None, take_pct: None, leverage: None, amount_usdc: None,
            take_r: None, auto_rules: None, entry_px: None, pullback_pct: None, partial_take: None, trailing_pct: None,
            twap_minutes: None, twap_randomize: None, hedge: None, hedge_share: None,
        }
    }
}

/// A share as a percent; one too big to count (a model's number) stays a share rather than taking the engine down.
fn percent(share: Decimal) -> Decimal { share.checked_mul(Decimal::ONE_HUNDRED).unwrap_or(Decimal::MAX) }

impl TradeIntent {
    pub fn is_hedge(&self) -> bool { self.hedge.is_some() || self.hedge_share.is_some() }

    /// Human-readable list of fields that differ from `old`.
    pub fn changes(&self, old: &TradeIntent) -> Vec<String> {
        let opt = |v: Option<Decimal>, f: &dyn Fn(Decimal) -> String| v.map_or("—".to_string(), f);
        let mut out = Vec::new();
        if self.asset != old.asset { out.push(format!("market {}", self.asset)) }
        if self.side != old.side { out.push(if self.side == Side::Long { "long" } else { "short" }.into()) }
        if self.risk_usd != old.risk_usd { out.push(format!("risk {}", opt(self.risk_usd, &Fmt::usd))) }
        if self.stop_pct != old.stop_pct { out.push(format!("stop {}", opt(self.stop_pct, &Fmt::pct))) }
        if self.take_pct != old.take_pct { out.push(format!("take {}", opt(self.take_pct, &Fmt::pct))) }
        if self.leverage != old.leverage { out.push(format!("leverage {}", opt(self.leverage, &|v| Fmt::num(v, 2) + "×"))) }
        if self.amount_usdc != old.amount_usdc { out.push(format!("money in the trade {}", opt(self.amount_usdc, &|v| Fmt::num(v, 2) + " USDC"))) }
        if self.take_r != old.take_r { out.push(format!("take {}", opt(self.take_r, &|v| Fmt::num(v, 2) + "R"))) }
        if self.auto_rules != old.auto_rules && self.auto_rules == Some(true) { out.push("the rest by the risk rules".into()) }
        if self.partial_take != old.partial_take && self.partial_take == Some(true) {
            out.push("take in parts: half at 1R, then stop to breakeven".into())
        }
        if self.trailing_pct != old.trailing_pct { out.push(format!("trailing {}", opt(self.trailing_pct, &Fmt::pct))) }
        if self.twap_minutes != old.twap_minutes || self.twap_randomize != old.twap_randomize {
            out.push(self.twap_minutes.map_or("one market order".into(), |m| {
                format!("TWAP over {}{}", Self::duration(m), if self.twap_randomize == Some(true) { ", randomized" } else { "" })
            }));
        }
        if self.is_hedge() && (self.hedge != old.hedge || self.hedge_share != old.hedge_share) {
            out.push(match (self.hedge, self.hedge_share) {
                (Some(h), _) => format!("hedge {} {}", Fmt::num(h, 6), self.asset),
                (None, Some(s)) if s < Decimal::ONE => format!("hedge {} of the {} held", Fmt::pct(percent(s)), self.asset),
                (None, Some(_)) => format!("hedge all the {} held", self.asset),
                (None, None) => String::new(),
            });
        }
        if self.entry_px != old.entry_px || self.pullback_pct != old.pullback_pct {
            out.push(match (self.entry_px, self.pullback_pct) {
                (Some(px), _) => format!("limit entry at {}", Fmt::price(px)),
                (None, Some(p)) => format!("entry on a {} pullback", Fmt::pct(p)),
                (None, None) => "market entry".into(),
            });
        }
        out
    }

    /// The plan in the router's own form («long SOL risk $0.3 stop 3% take 6% leverage 2x»), which IntentParser.apply
    /// reads back to the same plan: what a price alert tied to a plan runs to prepare it again.
    /// A plan missing its risk or its stop is filled by the risk rules, as «by the book» is; what the user named stays.
    /// «long ETH» alone would otherwise equal the empty plan (TradeIntent's defaults are ETH, long), which isn't drawn.
    pub fn started(&self) -> TradeIntent {
        let mut next = self.clone();
        // A hedge is sized by the coins held: the rules have no risk to fill.
        if next.auto_rules.is_none() && !self.is_hedge() && (next.risk_usd.is_none() || next.stop_pct.is_none()) { next.auto_rules = Some(true) }
        next
    }

    pub fn command(&self) -> String {
        let n = text;
        let name = self.asset.strip_prefix("xyz:").unwrap_or(&self.asset);
        if self.is_hedge() {
            let share = self.hedge_share.unwrap_or(Decimal::ONE);
            let mut parts = vec![match self.hedge {
                Some(h) => format!("hedge {} {name}", n(h)),
                None => format!("hedge {name}{}", if share < Decimal::ONE { format!(" {}%", n(percent(share))) } else { String::new() }),
            }];
            if let Some(v) = self.leverage { parts.push(format!("leverage {}x", n(v))) }
            if let Some(v) = self.stop_pct { parts.push(format!("stop {}%", n(v))) }
            return parts.join(" ");
        }
        let mut parts = vec![format!("{} {name}", if self.side == Side::Long { "long" } else { "short" })];
        if let Some(v) = self.risk_usd { parts.push(format!("risk ${}", n(v))) }
        if let Some(v) = self.stop_pct { parts.push(format!("stop {}%", n(v))) }
        if let Some(v) = self.take_pct { parts.push(format!("take {}%", n(v))) } else if let Some(v) = self.take_r { parts.push(format!("take {}R", n(v))) }
        if let Some(v) = self.leverage { parts.push(format!("leverage {}x", n(v))) }
        if let Some(v) = self.amount_usdc { parts.push(format!("{} USDC", n(v))) }
        if self.auto_rules == Some(true) { parts.push("by the book".into()) }
        // «pullback N%»: the form the parser reads first.
        if let Some(v) = self.pullback_pct { parts.push(format!("pullback {}%", n(v))) } else if let Some(v) = self.entry_px { parts.push(format!("limit {}", n(v))) }
        if let Some(v) = self.trailing_pct { parts.push(format!("trailing {}%", n(v))) }
        if self.partial_take == Some(true) { parts.push("take in parts".into()) }
        if let Some(v) = self.twap_minutes { parts.push(format!("over {v} min")) }
        if self.twap_randomize == Some(true) { parts.push("randomize".into()) }
        parts.join(" ")
    }

    /// «2 h», «90 min», «3 d»: a TWAP's running time.
    pub fn duration(minutes: i64) -> String {
        if minutes % 1440 == 0 { format!("{} d", minutes / 1440) } else if minutes % 60 == 0 { format!("{} h", minutes / 60) } else { format!("{minutes} min") }
    }
}

pub struct Fmt;

impl Fmt {
    /// «$1,234.50», «−$0.25».
    pub fn usd(d: Decimal) -> String {
        let text = digits(d.abs(), 2, 2);
        format!("{}{text}", if d.is_sign_negative() && !d.is_zero() && text != "0.00" { "−$" } else { "$" })
    }

    /// Up to `max_fraction` places (Swift's default is 2). A coin amount (6+ places) keeps four significant digits
    /// however small: 0.00001196 UBTC, not 0.000012 — the card's rate, worked out from the shown amount, came out 9% off.
    pub fn num(d: Decimal, max_fraction: u32) -> String {
        let mut fraction = max_fraction;
        if max_fraction >= 6 && !d.is_zero() {
            if let Some(mut scaled) = d.abs().checked_mul(crate::foundation::pow10(max_fraction)) {
                while scaled < Decimal::from(1000) && fraction < 18 {
                    scaled *= Decimal::TEN;
                    fraction += 1;
                }
            }
        }
        digits(d, 0, fraction)
    }

    /// Said on a swap's card when fees take more than a tenth of what's paid (2026-10-08: $0.18 of a $1 UBTC buy went to
    /// Relay): «fees take ≈ $0.18 (18%)». `worth` is what arrives in dollars after the fees and gas.
    pub fn fee_warning(paid: Decimal, worth: Decimal) -> Option<String> {
        let lost = paid.checked_sub(worth)?;
        if paid <= Decimal::ZERO || worth <= Decimal::ZERO || lost <= paid / Decimal::TEN { return None }
        let share = (lost / paid).checked_mul(Decimal::ONE_HUNDRED)?;
        Some(format!("⚠️ fees take ≈ {} ({}%): a bigger amount loses less", Self::usd(lost), Self::num(share, 0)))
    }

    /// Six significant digits at most, grouped: «2,650.5», «1,234,570», «0.0000123457».
    pub fn price(d: Decimal) -> String {
        if d.is_zero() { return "0".into() }
        let mantissa = d.abs().normalize();
        let places = mantissa.mantissa().to_string().len() as i64 - 1 - mantissa.scale() as i64;   // the leading digit's power of ten
        let scale = 5 - places;
        if scale >= 0 { return digits(rounded(d, scale as u32, Rounding::Bankers), 0, scale as u32) }
        let step = crate::foundation::pow10((-scale) as u32);
        digits(rounded(d / step, 0, Rounding::Bankers).checked_mul(step).unwrap_or(d), 0, 0)
    }

    pub fn pct(d: Decimal) -> String { Self::num(d, 2) + "%" }

    /// «Oct 6 at 9:00 AM», in this computer's time zone: no year, as en_US writes it on the Mac (a narrow no-break
    /// space before AM).
    pub fn date(d: Date) -> String { d.with_timezone(&Local).format("%b %-d at %-I:%M\u{202f}%p").to_string() }

    /// «9:00 PM»: the second end of a span on the same day.
    pub fn time(d: Date) -> String { d.with_timezone(&Local).format("%-I:%M\u{202f}%p").to_string() }

    /// A shown line with every number in it but a percentage as dots («worth ••• · +4.2%»): the balance hidden.
    pub fn dots(line: &str) -> String { replacing(line, r"[−+-]?\$?\d[\d,]*(?:\.\d+)?(?![\d.,]*%)", |_| "•••".into()) }
}

/// en_US with ICU's rounding (half to even): grouped thousands, `min`…`max` places, a minus on a negative that
/// rounds to zero too («-0», as Swift prints it).
fn digits(d: Decimal, min: u32, max: u32) -> String {
    let text = rounded(d.abs(), max, Rounding::Bankers).to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut fraction = fraction.trim_end_matches('0').to_string();
    while (fraction.len() as u32) < min { fraction.push('0') }
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 { grouped.push(',') }
        grouped.push(c);
    }
    format!("{}{grouped}{}{fraction}", if d.is_sign_negative() && !d.is_zero() { "-" } else { "" }, if fraction.is_empty() { "" } else { "." })
}

/// Deterministic parsing of the phrases a trader actually types. The cloud model only sees what this misses.
pub struct IntentParser;

impl IntentParser {
    const FALLBACK_MARKETS: [&str; 3] = ["BTC", "ETH", "SOL"];
    /// Words for markets that are not tickers: commodities on HIP-3 exchanges. Brent before oil: «brent oil» is Brent.
    /// Names people say instead of tickers. Not «Solana»: «с соланы» in a trade names the network.
    const ALIASES: [(&str, &str); 6] = [
        ("брент|brent", "BRENTOIL"), (r"нефт|\boil\b|\bwti\b|crude", "CL"), (r"золот|\bgold\b", "GOLD"), ("серебр|silver", "SILVER"),
        (r"\bэфир\w*|\bethereum\b|\bэфириум\w*", "ETH"), (r"биткоин\w*|биткойн\w*|\bбиток\w*|\bbitcoin\b", "BTC"),
    ];
    const NOT_TICKERS: [&str; 10] = ["LONG", "SHORT", "STOP", "TAKE", "RISK", "SL", "TP", "USDC", "USD", "LEV"];
    /// Hyperliquid tickers that are everyday words: in lowercase they are words («move stop», «not now»), not markets.
    const EVERYDAY_WORDS: [&str; 39] = [
        "ACE", "ALT", "APE", "AR", "BABY", "BANANA", "BLUR", "CAKE", "CHIP", "DASH", "DOT", "GAS", "GOAT", "GRASS", "HYPER", "IO",
        "LAYER", "LINK", "LIT", "ME", "MET", "MON", "MOVE", "NEAR", "NIL", "NOT", "PEOPLE", "PROVE", "PUMP", "RENDER", "SAND", "SKY",
        "STABLE", "SUPER", "TURBO", "USELESS", "USUAL", "VIRTUAL", "ZEN",
    ];

    /// «xyz:CL» → «WTI oil (xyz:CL)» for the plan's headings.
    pub fn pretty(asset: &str) -> String {
        let name = asset.split_once(':').and_then(|(_, ticker)| match ticker {
            "CL" => Some("WTI oil"), "BRENTOIL" => Some("Brent oil"), "GOLD" => Some("gold"), "SILVER" => Some("silver"), "TSLA" => Some("Tesla"),
            "NVDA" => Some("Nvidia"), "GOOGL" => Some("Google"), "META" => Some("Meta"), "AAPL" => Some("Apple"), "MSFT" => Some("Microsoft"),
            "AMZN" => Some("Amazon"), "MU" => Some("Micron"), "COIN" => Some("Coinbase"), "CRCL" => Some("Circle"), "SNDK" => Some("Sandisk"),
            "SP500" => Some("S&P 500"), "XYZ100" => Some("tech 100 index"), _ => None,
        });
        name.map_or(asset.into(), |name| format!("{name} ({asset})"))
    }

    /// Returns the updated intent, or nil when nothing in `text` was recognised.
    pub fn apply(text: &str, intent: &TradeIntent, markets: &HashSet<String>) -> Option<TradeIntent> {
        let q = Self::plain_numbers(&text.to_lowercase());
        let trade = has(&q, r"\b(?:лонг|шорт|long|short)");
        // «sell SOL for 10 USDC», «продай сол на 10 баксов», «buy SOL for 5 USDC every 0 days», «buy USDT for 750 USDC»:
        // never a long of that many USDC — the last one drew a 5× ETH long for a USDC → USDT swap.
        if has(&q, r"\bsell\b|прода[йтв]|\bbuy\b|куп|покупа") && !trade && has(&q, r"\$\s*\d|\d\s*(?:usdc|usdg|\$|долл|dollars?|bucks|бакс)") {
            return None;
        }
        // A money move the wallet parser didn't take: never a trade plan of the ticker it names. A stop or a take being
        // moved stays the plan's. An amount, or «all»: a bare «MOVE» is the ticker being looked up, not a move.
        if !trade && !has(&q, r"\b(?:stop|take|tp|sl|стоп\w*|тейк\w*)\b")
            && (has(&q, r"^\s*(?:move|swap|exchange|convert|bridge|send|transfer|deposit|withdraw|lend|stake|receive|top\s*up|переве\w*|перевод\w*|отправ\w*|помен\w*|обмен\w*|свап\w*|своп\w*|конверт\w*|выведи|вывод\w*|полож\w*|внеси|пополн\w*|задепоз\w*|бридж\w*)\b")
                && has(&q, r"\d|\ball\b|\bвс[её]\b|\bвесь\b")
                || has(&q, r"^\s*(?:buy|sell|купи\w*|прода\w*)\b.*\b(?:on|на|в|во)\s+(?:the\s+)?(?:solana|base|arbitrum|polygon|ethereum|mainnet|optimism|hyperevm|robinhood|tempo|bnb|bsc|binance|avalanche|avax|sui|tron|солан\w*|бейс\w*|арбитрум\w*|полигон\w*|эфириум\w*|оптимизм\w*|хайперевм\w*|робинхуд\w*|темпо\w*|бнб|бсц|бинанс\w*|аваланч\w*)\b"))
        {
            return None;
        }
        let mut next = intent.clone();
        let mut hit = false;
        // «hedge SOL», «захеджируй половину эфира»: a plan of its own, sized by what the wallet holds, never a change to the
        // plan on the canvas; a long or a short after it is a trade again.
        // «не хеджируй», «сними хедж»: never a new hedge, and nothing else either; «long SOL, no hedge» is the long.
        let unhedge = has(&q, r"\bне\s+(?:за)?хедж|сним\w*\s+хедж|убери\s+хедж|без\s+хедж|\bno\s+hedg|\bdon'?t\s+hedg|\bunhedg|\bwithout\s+(?:a\s+)?hedg");
        if unhedge && !trade { return None }
        let hedging = !unhedge && has(&q, r"\bhedg|хедж");
        // A dollar amount isn't a hedge's size: it counts coins, so the model asks.
        if hedging && has(&q, r"\$\s*\d|\d\s*(?:\$|usdc|usd\b|долл|бакс)") { return None }
        if hedging { next = TradeIntent::default() }
        let mut named = false;

        if has(&q, r"\b(?:лонг|long)") { next.side = Side::Long; hit = true }
        if has(&q, r"\b(?:шорт|short)") { next.side = Side::Short; hit = true }
        if !hedging && trade { next.hedge = None; next.hedge_share = None }

        let known: HashSet<&str> = if markets.is_empty() { Self::FALLBACK_MARKETS.into() } else { markets.iter().map(String::as_str).collect() };
        // A ticker on the main exchange, else the same ticker on a HIP-3 one («xyz:TSLA»).
        let market = |ticker: &str| -> Option<String> {
            if known.contains(ticker) { return Some(ticker.into()) }
            // Coins priced in fractions of a cent trade by the thousand: PEPE is «kPEPE», BONK «kBONK».
            let thousand = format!("k{ticker}");
            if known.contains(thousand.as_str()) { return Some(thousand) }
            if let Some(k) = ticker.strip_prefix('K').map(|rest| format!("k{rest}")).filter(|k| known.contains(k.as_str())) { return Some(k) }
            let mut sorted: Vec<&str> = known.iter().copied().collect();
            sorted.sort();
            sorted.into_iter().find(|m| m.ends_with(&format!(":{ticker}"))).map(str::to_string)
        };
        if let Some(m) = first(&q, r"\$([a-z][a-z0-9]{1,9})\b") {
            let ticker = group(&m, 1).to_uppercase();
            next.asset = market(&ticker).unwrap_or(ticker);
            hit = true;
            named = true;
        } else if let Some((_, name)) = Self::ALIASES.iter().find(|(p, _)| has(&q, p)) {
            next.asset = market(name).unwrap_or(format!("xyz:{name}"));
            hit = true;
            named = true;
        } else {
            // Typed as a ticker («ETH», «NOW») a word counts on any exchange; in lowercase only a main-exchange ticker
            // that isn't an everyday word, so «what is open now» or «move stop» never name a market.
            for word in split_chars(text, |g| !is_letter(g) && !is_number(g)) {
                let ticker = word.to_uppercase();
                if count(&ticker) < 2 || Self::NOT_TICKERS.contains(&ticker.as_str()) { continue }
                // «kPEPE» as Hyperliquid spells it, «kpepe», «KPEPE»: the thousand-unit market.
                let thousand = ticker.strip_prefix('K').map(|rest| format!("k{rest}")).filter(|k| known.contains(k.as_str()));
                let name = if word == ticker {
                    market(&ticker)
                } else if known.contains(ticker.as_str()) && !Self::EVERYDAY_WORDS.contains(&ticker.as_str()) {
                    Some(ticker)
                } else {
                    thousand
                };
                if let Some(name) = name {
                    next.asset = name;
                    hit = true;
                    named = true;
                    break;
                }
            }
        }

        // «риск $10», «риск на сделку — не больше 10$». A percent after the number is not dollars.
        if let Some(v) = Self::number(&q, r"(?:риск|risk)[^\d%]{0,30}?(\d+(?:[.,]\d+)?)(?!\d|[.,]\d|\s*%)") { next.risk_usd = Some(v); hit = true }
        if let Some(v) = Self::number(&q, r"(?:стоп|stop|\bsl\b)[^\d]{0,15}?(\d+(?:[.,]\d+)?)\s*%") { next.stop_pct = Some(v); hit = true }
        // Between the word and the number at most one short word («тейк на 6%», «take profit at 6%»): «take in parts on a 2%
        // pullback» has no take percent.
        if let Some(v) = Self::number(&q, r"(?:тейк|take|\btp\b)(?:[- ]?(?:profit|профит)\w*)?[^\d\p{L}]*(?:\p{L}{1,3}[^\d\p{L}]+)?(\d+(?:[.,]\d+)?)\s*%") {
            next.take_pct = Some(v);
            hit = true;
        }
        // «плечо 2», «2x», «3х» (Cyrillic х), «2×»
        if let Some(v) = Self::number(&q, r"(?:плеч|leverage|\blev\b)[^\d]{0,10}?(\d+(?:[.,]\d+)?)")
            .or_else(|| Self::number(&q, r"(\d+(?:[.,]\d+)?)\s*[xх×](?!\p{L})"))
        {
            next.leverage = Some(v);
            hit = true;
        }
        if let Some(v) = Self::number(&q, r"(\d+(?:[.,]\d+)?)\s*usdc") { next.amount_usdc = Some(v); hit = true }
        // «тейк 2R», «1:3», «один к трём» are ratios to the stop, not percents.
        if let Some(v) = Self::number(&q, r"(?:^|[^\d.,])(\d+(?:[.,]\d+)?)\s*r\b").or_else(|| Self::number(&q, r"\b1\s*[:к]\s*(\d+(?:[.,]\d+)?)")) {
            next.take_r = Some(v);
            next.take_pct = None;
            hit = true;
        }
        if has(&q, r"как\s+учат|как\s+советуют|по\s+правилам|как\s+(?:лучше|правильно|положено)|по\s+уму|грамотн|риск[- ]?менеджмент|by\s+the\s+(?:book|rules)|best\s+practice|properly|risk[- ]?management|like\s+(?:the\s+)?pros") {
            next.auto_rules = Some(true);
            hit = true;
        }
        // «вход на откате 2%», «подожди откат 2%»; «лимит 2650», «вход 2650»; «по рынку» clears both. After the word at most
        // one short word, so «on a 2% pullback trailing 3%» is a 2% pullback and «stop 2% pullback 3%» a 3% one.
        if let Some(v) = Self::number(&q, r"(?:откат\w*|pullback|dip|retrace\w*)[^\d\p{L}]*(?:\p{L}{1,3}[^\d\p{L}]+)?(\d+(?:[.,]\d+)?)\s*%")
            .or_else(|| Self::number(&q, r"(\d+(?:[.,]\d+)?)\s*%\s*(?:pullback|dip|retrace\w*)"))
        {
            next.pullback_pct = Some(v);
            next.entry_px = None;
            hit = true;
        } else if let Some(v) = Self::number(&q, r"(?:лимит\w*|вход\w*|limit|entry|enter)\s*(?:по|на|at)?\s*\$?(\d+(?:[.,]\d+)?)(?!\s*%|\d|[.,]\d)") {
            next.entry_px = Some(v);
            next.pullback_pct = None;
            hit = true;
        }
        if has(&q, r"по\s+рынку|market") { next.entry_px = None; next.pullback_pct = None; hit = true }
        if has(&q, r"частям|частичн|scale\s*out|in\s+parts|partial") { next.partial_take = Some(true); hit = true }
        if let Some(v) = Self::number(&q, r"(?:трейл\w*|скользящ\w*|trail\w*)(?:[- ]?(?:стоп\w*|stop))?[^\d]{0,6}?(\d+(?:[.,]\d+)?)\s*%") {
            next.trailing_pct = Some(v);
            hit = true;
        }
        // «over 2 h», «за 2 часа», «в течение 30 минут», «twap 90 min»: the entry as one TWAP. Not «in 2 hours» or «через
        // 2 часа»: those say when, not how long. «one order» / «одним ордером» goes back to a single order.
        if let Some(m) = first(&q, r"(?:\bover|\bduring|\btwap|(?:^|\s)за|в\s+течени\w*)\s+(?:the\s+next\s+)?(\d+(?:[.,]\d+)?)\s*(минут\w*|мин\b|min\w*|m\b|час\w*|ч\b|hours?\b|hrs?\b|h\b|дн\w*|день|сут\w*|days?\b|d\b)")
            && let Some(v) = decimal(&group(&m, 1).replace(',', "."))
        {
            let u = group(&m, 2);
            let unit = if u.starts_with('m') || u.starts_with('м') { 1 } else if u.starts_with('h') || u.starts_with('ч') { 60 } else { 1440 };
            // ponytail: a count past Int's range is Int.max here; Swift's intValue of it is undefined.
            next.twap_minutes = Some(v.checked_mul(Decimal::from(unit)).and_then(|m| rounded(m, 0, Rounding::Plain).to_i64()).unwrap_or(i64::MAX));
            hit = true;
        } else if has(&q, r"(?:^|\s)за\s+(?:один\s+)?час\b|\bover\s+(?:an|one)\s+hour\b") {
            next.twap_minutes = Some(60);
            hit = true;
        } else if has(&q, r"(?:^|\s)за\s+полчаса|\bover\s+half\s+an\s+hour") {
            next.twap_minutes = Some(30);
            hit = true;
        }
        // Only about a TWAP: «a random coin» elsewhere is no change to a plan.
        if next.twap_minutes.is_some() && has(&q, r"\brandomi[sz]\w*|\brandom\b|рандом\w*|случайн\w*") { next.twap_randomize = Some(true); hit = true }
        if intent.twap_minutes.is_some() && has(&q, r"\bone\s+(?:market\s+)?order\b|одн\w*\s+(?:рыночн\w*\s+)?ордер\w*|сразу\s+целиком") {
            next.twap_minutes = None;
            next.twap_randomize = None;
            hit = true;
        }
        if hedging {
            // Only a coin named: «хедж мне что-нибудь» is the model's to pick from what the wallet holds.
            if !named { return None }
            next.side = Side::Short;
            // «hedge SOL 30%», «хеджируй 30% эфира»: a share; «hedge 0.1 SOL»: coins; «half», else all of it.
            if let Some(v) = Self::number(&q, r"(?:hedg|хедж)\w*\s+(?:\S+\s+)?(\d+(?:[.,]\d+)?)\s*%").filter(|v| *v > Decimal::ZERO && *v <= Decimal::ONE_HUNDRED) {
                next.hedge_share = Some(v / Decimal::ONE_HUNDRED);
            } else if let Some(v) = Self::number(&q, r"(?:hedg|хедж)\w*\s+(\d+(?:[.,]\d+)?)(?![\d.,]|\s*%)") {
                next.hedge = Some(v);
            } else {
                next.hedge_share = Some(if has(&q, r"\bhalf\b|половин") { Decimal::new(5, 1) } else { Decimal::ONE });
            }
            hit = true;
        }
        // A hedge is sized by the coins and has no take: a risk, a take, money in the trade or a limit said for one would
        // be listed as a change and then do nothing.
        if next.is_hedge() {
            next.risk_usd = None;
            next.take_pct = None;
            next.take_r = None;
            next.amount_usdc = None;
            next.auto_rules = None;
            next.entry_px = None;
            next.pullback_pct = None;
            next.partial_take = None;
            next.trailing_pct = None;
            next.twap_minutes = None;
            next.twap_randomize = None;
        }

        hit.then_some(next)
    }

    /// Wording the parser cannot turn into numbers («тейк ближе», «плечо поменьше»). A message with it goes to the model
    /// whole, even if the parser recognised another part, so no part of it is silently dropped.
    pub fn needs_model(text: &str) -> bool {
        has(&text.to_lowercase(), r"ближе|дальше|больше|меньше|выше|ниже|шире|увелич|уменьш|удво|вдвое|в \d+ раза|половин|чуть|немного|closer|further|wider|tighter|double|half|bigger|smaller|higher|lower|a bit|slightly")
    }

    /// The market the text names («close ETH», «закрой нефть»), or nil when it names none.
    pub fn named_asset(text: &str, markets: &HashSet<String>) -> Option<String> {
        let probe = TradeIntent { asset: String::new(), ..TradeIntent::default() };
        Self::apply(text, &probe, markets).map(|i| i.asset).filter(|a| !a.is_empty())
    }

    /// A question to answer, not an order to carry out: «а где наш шорт по нефти?», «what did you do with ETH».
    pub fn is_question(text: &str) -> bool {
        let lower = text.to_lowercase();
        let q = lower.trim();
        q.ends_with('?')
            || has(q, r"^(?:а\s+|и\s+|so\s+|and\s+)?(?:где|что|сколько|почему|зачем|когда|какой|какая|какие|каким|куда\s+дел|кто|откуда|what|where|why|when|which|who|how\s+(?:much|many|is|are|did|was|were|do|come)|is\s+(?:my|the|there)|are\s+(?:my|there)|did\s+(?:you|i|we)|do\s+i|have\s+(?:i|you|we))\b")
    }

    /// Whether the text has a trading word in Russian or English. Without one, a ticker and an amount alone («compra SOL
    /// por 1 USDC») are too weak to trust: the message may be another language asking for something else.
    pub fn has_trade_words(text: &str) -> bool {
        has(&text.to_lowercase(), r"лонг|шорт|\blong\b|\bshort\b|\bhedg|хедж|риск|\brisk\b|стоп|\bstop|тейк|\btake\b|\btp\b|\bsl\b|плеч|leverage|\d\s*[xх×]\b|откат|pullback|лимит|limit|по\s+рынку|market|как\s+учат|by\s+the\s+(?:book|rules)|трейл|trail|частям|in\s+parts|scale\s*out|\d\s*r\b|\b1\s*[:к]\s*\d")
    }

    /// Numbers as the parsers read them: «2,500» and «1,234.56» lose their thousands commas and «.5» becomes «0.5»; a
    /// decimal comma («1,5», «0,500», «33,333%») stays for the parsers to read as a point, as AlertCommand.level does.
    pub(crate) fn plain_numbers(text: &str) -> String {
        let t = replacing(text, r"(^|[^\d.,])([1-9]\d{0,2}(?:,\d{3})+)(?!,?\d|\s*%)", |c| format!("{}{}", group(c, 1), group(c, 2).replace(',', "")));
        replacing(&t, r"(^|[^\w.,])\.(?=\d)", |c| format!("{}0.", group(c, 1)))
    }

    /// The pattern's first group as a number, a decimal comma read as a point.
    pub(crate) fn number(q: &str, pattern: &'static str) -> Option<Decimal> { decimal(&group(&first(q, pattern)?, 1).replace(',', ".")) }
}

/// Blocks a recovery phrase before it reaches the model, the chat history or a log.
pub struct SeedGuard;

impl SeedGuard {
    /// True when the text has 12+ consecutive BIP-39 words. Numbers and punctuation between them («1. abandon 2. ability
    /// …») do not break the run; any other word does.
    pub fn contains_mnemonic(text: &str) -> bool {
        let lower = text.to_lowercase();
        let mut run = 0;
        for word in split_chars(&lower, |g| !is_letter(g)) {
            run = if wallet::mnemonic::index(word).is_some() { run + 1 } else { 0 };
            if run >= 12 { return true }
        }
        false
    }

    /// True when the text holds a private key: a Solana keypair — 64 bytes in Base58 or as a JSON array, its second half
    /// the first half's public key, so a transaction's signature (64 bytes too) isn't one — or 32 bytes of hex (an EVM
    /// key) with a word for a key beside it: alone, 64 hex digits are as likely a transaction hash or a NEAR account.
    /// ponytail: an EVM key pasted with no such word passes; nothing in the hex itself tells it from a hash.
    pub fn contains_private_key(text: &str) -> bool {
        let keypair = |b: &[u8]| b.len() == 64 && wallet::keys::ed25519_public(&b[..32]).is_ok_and(|public| public[..] == b[32..]);
        for m in all(text, r"\[\s*\d{1,3}(?:\s*,\s*\d{1,3}){63}\s*\]") {
            let bytes: Vec<u8> = split_chars(group(&m, 0), |g| !is_number(g)).iter().filter_map(|s| s.parse().ok()).collect();
            if bytes.len() == 64 && keypair(&bytes) { return true }
        }
        let key_word = has(&text.to_lowercase(), r"private|secret|\bkey\b|приват|секрет|ключ");
        // Character.isHexDigit: ASCII's and their fullwidth forms.
        let hex_digit = |g: &str| g.chars().next().is_some_and(|c| c.is_ascii_hexdigit() || matches!(c, '０'..='９' | 'Ａ'..='Ｆ' | 'ａ'..='ｆ'));
        for word in split_chars(text, |g| !is_letter(g) && !is_number(g)) {
            let hex = word.strip_prefix("0x").unwrap_or(word);
            if key_word && count(hex) == 64 && all_chars(hex, hex_digit) { return true }
            if (86..=88).contains(&count(word)) && wallet::keys::base58::decode(word).is_ok_and(|b| keypair(&b)) { return true }
        }
        false
    }
}

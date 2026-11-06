//! Part of `Intent.swift`, what round 1 needed (the Intent round completes this file): the trade a chat asks for
//! (`TradeIntent`, its fields), `Fmt`'s money, amount and date texts, and `IntentParser.namedAsset` with the part of
//! `apply` it runs.

use std::collections::HashSet;

use chrono::Local;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::foundation::{Date, Rounding, count, first, group, has, is_letter, is_number, number, replacing, rounded, split_chars};

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

impl TradeIntent {
    pub fn is_hedge(&self) -> bool { self.hedge.is_some() || self.hedge_share.is_some() }
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

    /// «Oct 6 at 9:00 AM», in this computer's time zone: no year, as en_US writes it on the Mac (a narrow no-break
    /// space before AM).
    pub fn date(d: Date) -> String { d.with_timezone(&Local).format("%b %-d at %-I:%M\u{202f}%p").to_string() }
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

/// Deterministic parsing of the phrases a trader actually types.
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

    /// The market the text names («close ETH», «закрой нефть»), or nil when it names none: `apply` on an empty plan,
    /// its asset.
    // pending: the rest of IntentParser.apply (risk, stop, take, leverage, TWAP, hedge sizes) comes with the Intent round.
    pub fn named_asset(text: &str, markets: &HashSet<String>) -> Option<String> {
        let q = Self::plain_numbers(&text.to_lowercase());
        let trade = has(&q, r"\b(?:лонг|шорт|long|short)");
        // «sell SOL for 10 USDC», «продай сол на 10 баксов», «buy USDT for 750 USDC»: never a long of that many USDC.
        if has(&q, r"\bsell\b|прода[йтв]|\bbuy\b|куп|покупа") && !trade && has(&q, r"\$\s*\d|\d\s*(?:usdc|usdg|\$|долл|dollars?|bucks|бакс)") {
            return None;
        }
        // A money move the wallet parser didn't take: never a trade plan of the ticker it names. A stop or a take being
        // moved stays the plan's.
        if !trade && !has(&q, r"\b(?:stop|take|tp|sl|стоп\w*|тейк\w*)\b")
            && (has(&q, r"^\s*(?:move|swap|exchange|convert|bridge|send|transfer|deposit|withdraw|lend|stake|receive|top\s*up|переве\w*|перевод\w*|отправ\w*|помен\w*|обмен\w*|свап\w*|своп\w*|конверт\w*|выведи|вывод\w*|полож\w*|внеси|пополн\w*|задепоз\w*|бридж\w*)\b")
                && has(&q, r"\d|\ball\b|\bвс[её]\b|\bвесь\b")
                || has(&q, r"^\s*(?:buy|sell|купи\w*|прода\w*)\b.*\b(?:on|на|в|во)\s+(?:the\s+)?(?:solana|base|arbitrum|polygon|ethereum|mainnet|optimism|hyperevm|robinhood|tempo|bnb|bsc|binance|avalanche|avax|sui|tron|солан\w*|бейс\w*|арбитрум\w*|полигон\w*|эфириум\w*|оптимизм\w*|хайперевм\w*|робинхуд\w*|темпо\w*|бнб|бсц|бинанс\w*|аваланч\w*)\b"))
        {
            return None;
        }
        // «не хеджируй», «сними хедж»: nothing; a dollar amount isn't a hedge's size.
        let unhedge = has(&q, r"\bне\s+(?:за)?хедж|сним\w*\s+хедж|убери\s+хедж|без\s+хедж|\bno\s+hedg|\bdon'?t\s+hedg|\bunhedg|\bwithout\s+(?:a\s+)?hedg");
        if unhedge && !trade { return None }
        if !unhedge && has(&q, r"\bhedg|хедж") && has(&q, r"\$\s*\d|\d\s*(?:\$|usdc|usd\b|долл|бакс)") { return None }

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
            return Some(market(&ticker).unwrap_or(ticker));
        }
        if let Some((_, name)) = Self::ALIASES.iter().find(|(p, _)| has(&q, p)) {
            return Some(market(name).unwrap_or(format!("xyz:{name}")));
        }
        // Typed as a ticker («ETH», «NOW») a word counts on any exchange; in lowercase only a main-exchange ticker that
        // isn't an everyday word, so «what is open now» or «move stop» never name a market.
        for word in split_chars(text, |g| !is_letter(g) && !is_number(g)) {
            let ticker = word.to_uppercase();
            if count(&ticker) < 2 || Self::NOT_TICKERS.contains(&ticker.as_str()) { continue }
            let thousand = ticker.strip_prefix('K').map(|rest| format!("k{rest}")).filter(|k| known.contains(k.as_str()));
            let name = if word == ticker {
                market(&ticker)
            } else if known.contains(ticker.as_str()) && !Self::EVERYDAY_WORDS.contains(&ticker.as_str()) {
                Some(ticker)
            } else {
                thousand
            };
            if name.is_some() { return name }
        }
        None
    }

    /// Numbers as the parsers read them: «2,500» and «1,234.56» lose their thousands commas and «.5» becomes «0.5»; a
    /// decimal comma («1,5», «0,500», «33,333%») stays for the parsers to read as a point, as AlertCommand.level does.
    pub(crate) fn plain_numbers(text: &str) -> String {
        let t = replacing(text, r"(^|[^\d.,])([1-9]\d{0,2}(?:,\d{3})+)(?!,?\d|\s*%)", |c| format!("{}{}", group(c, 1), group(c, 2).replace(',', "")));
        replacing(&t, r"(^|[^\w.,])\.(?=\d)", |c| format!("{}0.", group(c, 1)))
    }
}

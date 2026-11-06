//! Company names people say, to Robinhood Stock Token symbols (`StockNames.swift`), and Ondo's tokenized stocks.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::foundation::{all_chars, count, has, is_letter, split_chars};
use crate::stablecoins::DOLLARS;

/// Company names people say, to Robinhood Stock Token symbols. The token address itself always comes from
/// Robinhood's registry and is checked on-chain; this only turns «SpaceX» or «тесла» into «SPCX» / «TSLA».
const NAMES: [(&str, &str); 14] = [
    (r"spacex|space\s*x\b|спейс\s*икс|спейсикс|спэйс", "SPCX"), (r"\btesla\b|тесл\w*", "TSLA"),
    (r"nvidia|нвиди\w*|нвидия", "NVDA"), (r"\bapple\b|эппл|эпл\b", "AAPL"), (r"microsoft|майкрософт", "MSFT"),
    (r"amazon|амазон", "AMZN"), (r"google|alphabet|гугл", "GOOGL"), (r"\bmeta\b|facebook|фейсбук", "META"),
    (r"coinbase|коинбейс", "COIN"), (r"palantir|палантир", "PLTR"), (r"netflix|нетфликс", "NFLX"),
    (r"\bamd\b", "AMD"), (r"robinhood\s+(?:stock|shares?|акци)|акци\w*\s+робинхуд", "HOOD"), (r"\bcircle\b", "CRCL"),
];

/// Words that look like tickers but aren't stocks here.
static NOT_STOCKS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    ["USDC", "USDG", "USD", "ETH", "SOL", "BTC", "JUP", "HYPE", "USDT", "SPOT", "BUY", "SELL", "FOR", "ON", "THE", "ALL"]
        .map(String::from).into_iter().chain(DOLLARS.iter().cloned()).collect()
});

pub fn symbol(text: &str) -> Option<String> {
    let q = text.to_lowercase();
    if let Some((_, ticker)) = NAMES.iter().find(|(p, _)| has(&q, p)) { return Some(ticker.to_string()) }
    // A ticker written as one: «buy SPCX stock», «$NVDA».
    split_chars(text, |g| !is_letter(g) && g != "$").into_iter()
        .map(|word| word.strip_prefix('$').unwrap_or(word))
        .find(|w| (2..=5).contains(&count(w)) && *w == w.to_uppercase() && all_chars(w, |g| g.is_ascii() && is_letter(g)) && !NOT_STOCKS.contains(*w))
        .map(Into::into)
}

/// Ondo Global Markets' tokenized US stocks and ETFs on Solana (NVDAon, SPYon…: Token-2022 mints ending in «ondo», each
/// Jupiter-verified and filled by JupiterZ's market makers, whom Ondo mints and redeems for) and USDY, its dollar
/// earning the US Treasury rate. Checked 2026-10-29 against Jupiter's list and docs.ondo.finance; on chain the stocks'
/// mint authority is 9foMHsSDq7nMg4WPusSz9eY7tyxyukqborA8GyU5cUxD, USDY's 8n2uiRUd…, all frozen by 51QVCuHf…. The
/// suffix is a vanity anyone can grind: a token counts as Ondo's only Jupiter-verified too (SpotAsset.ondo).
pub struct Ondo;

impl Ondo {
    pub const USDY: &str = "A1KLoBrKBde8Ty9qtNQUtq3C2ortoC3u7twggz7sEto6";

    pub fn issued(mint: &str) -> bool { mint == Self::USDY || mint.ends_with("ondo") }

    /// Ondo's terms, said before Confirm. Its freeze and mint authorities are an issuer's, not a scam's sign.
    pub fn terms(russian: bool) -> &'static str {
        if russian {
            "Ondo не продаёт свои токены жителям США, Канады, России и Беларуси, а в ЕЭЗ, Британии, Сингапуре, Гонконге, Швейцарии, Бразилии и Малайзии — только квалифицированным инвесторам. Погашение у самого Ondo — после KYC."
        } else {
            "Ondo doesn't offer its tokens to residents of the US, Canada, Russia or Belarus, and in the EEA, the UK, Singapore, Hong Kong, Switzerland, Brazil and Malaysia only to qualified investors. Redeeming at Ondo itself needs KYC."
        }
    }
}

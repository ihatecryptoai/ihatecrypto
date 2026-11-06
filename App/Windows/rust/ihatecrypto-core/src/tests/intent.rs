use std::collections::HashSet;

use rust_decimal::Decimal;

use super::{d, set};
use crate::intent::{Fmt, IntentParser, SeedGuard, Side, TradeIntent};
use crate::position_command::PositionCommand;

fn apply(text: &str, to: &TradeIntent) -> Option<TradeIntent> { IntentParser::apply(text, to, &HashSet::new()) }

fn fresh(text: &str) -> Option<TradeIntent> { apply(text, &TradeIntent::default()) }

// MARK: Parser (IhatecryptoCoreTests)

#[test]
fn parses_full_trade_phrase() {
    let i = fresh("лонг ETH риск $10 стоп 3% тейк 6% плечо 2x").unwrap();
    assert_eq!(i.side, Side::Long);
    assert_eq!(i.asset, "ETH");
    assert_eq!(i.risk_usd, Some(d("10")));
    assert_eq!(i.stop_pct, Some(d("3")));
    assert_eq!(i.take_pct, Some(d("6")));
    assert_eq!(i.leverage, Some(d("2")));
    assert_eq!(i.amount_usdc, None);
}

#[test]
fn parses_russian_variants() {
    let i = fresh("Шорт $BTC, риск на сделку — не больше 15$, стоп-лосс 2,5%, 3х").unwrap();
    assert_eq!(i.side, Side::Short);
    assert_eq!(i.asset, "BTC");
    assert_eq!(i.risk_usd, Some(d("15")));
    assert_eq!(i.stop_pct, Some(d("2.5")));
    assert_eq!(i.leverage, Some(d("3")));
}

#[test]
fn percent_risk_is_not_dollars() { assert_eq!(fresh("риск 5%").and_then(|i| i.risk_usd), None) }

#[test]
fn bare_ticker_must_be_a_known_market() {
    let i = IntentParser::apply("лонг hype", &TradeIntent::default(), &set(&["HYPE", "ETH"])).unwrap();
    assert_eq!(i.asset, "HYPE");
    assert_eq!(IntentParser::apply("лонг hype", &TradeIntent::default(), &set(&["ETH"])).map(|i| i.asset).as_deref(), Some("ETH"));
}

#[test]
fn vague_edit_is_left_for_the_model() {
    assert_eq!(fresh("тейк ближе"), None);
    assert_eq!(fresh("привет"), None);
}

#[test]
fn explicit_source_amount() { assert_eq!(fresh("возьми 100 USDC").and_then(|i| i.amount_usdc), Some(d("100"))) }

/// Swift's `\b` breaks between two characters that aren't a word's too, but not inside a run of spaces (probed with
/// swiftc, 2026-11-06): «1.5 × the plan's» has a trade word («\d\s*[xх×]\b»), which it lost before foundation knew it.
#[test]
fn a_boundary_between_two_non_word_characters_is_swifts() {
    use crate::foundation::has;
    assert!(IntentParser::has_trade_words("would lose $6.00, more than 1.5 × the plan's $3.00"));
    assert!(has("1.5 × the", r"×\b") && has("a × b", r"\b×") && has("$ 5", r"\$\b") && has("($)", r"\(\b\$") && has("  x", r"^\b") && has("x  ", r"\b$"));
    assert!(has("a, b", r",\b") && has("a--b", r"-\b-") && has("a\t b", r"\t\b ") && has("«a»", r"»\b") && has("", r"\b"));
    assert!(!has("a  b", r"a \b b") && !has("a_b", r"a\b_"));
}

#[test]
fn relative_wording_goes_to_the_model_whole() {
    assert!(IntentParser::needs_model("тейк ближе и плечо 3x"));
    assert!(IntentParser::needs_model("плечо поменьше"));
    assert!(!IntentParser::needs_model("лонг ETH риск $10 стоп 3% тейк 6% плечо 2x"));
}

// pending: rulesFillStopTakeAndRiskWhenAsked: Plan.resolve's stop, take, risk and notes (round 3).
#[test]
fn rules_fill_stop_take_and_risk_when_asked() {
    let i = fresh("шорт ETH, тейк и стоп как учат").unwrap();
    assert!(i.side == Side::Short && i.auto_rules == Some(true) && i.stop_pct.is_none());
    let exits = fresh("тейк частями, трейлинг 2%").unwrap();
    assert!(exits.partial_take == Some(true) && exits.trailing_pct == Some(d("2")));
    let i = apply("стоп 2% тейк 3R", &i).unwrap();
    assert!(i.take_r == Some(d("3")) && i.take_pct.is_none());
    assert_eq!(fresh("тейк 1:2").and_then(|i| i.take_r), Some(d("2")));
}

// pending: limitEntryWaitsForAPullback: Plan.resolve and Plan.evaluate of the limit (round 3).
#[test]
fn limit_entry_waits_for_a_pullback() {
    let i = fresh("лонг ETH риск $10 стоп 3% тейк 6% на откате 2%").unwrap();
    assert!(i.pullback_pct == Some(d("2")) && i.entry_px.is_none());
    let i = apply("лимит 2050", &i).unwrap();
    assert!(i.entry_px == Some(d("2050")) && i.pullback_pct.is_none());
    assert_eq!(apply("по рынку", &i).unwrap().entry_px, None);
}

/// A limit «in parts» is what leaves the half at 1R to the guardian (partialOrderPending), in both languages.
#[test]
fn a_limit_taken_in_parts_waits_for_its_half() {
    for phrase in ["long ETH risk $10 stop 3% limit 1960 take in parts", "лонг ETH риск $10 стоп 3% лимит 1960 частями"] {
        let i = IntentParser::apply(phrase, &TradeIntent::default(), &set(&["ETH"])).unwrap();
        assert!(i.entry_px == Some(d("1960")) && i.partial_take == Some(true) && i.side == Side::Long, "{phrase}");
    }
}

#[test]
fn risk_followed_by_a_comma_is_still_read() {
    // Found live: «риск $1, стоп…» lost the risk because the comma looked like a decimal separator.
    let cl = set(&["xyz:CL", "ETH"]);
    assert_eq!(IntentParser::apply("шорт нефти риск $1, стоп и тейк как учат", &TradeIntent::default(), &cl).unwrap().risk_usd, Some(d("1")));
    assert_eq!(fresh("риск 1,5$ стоп 3%").unwrap().risk_usd, Some(d("1.5")));
    assert_eq!(IntentParser::apply("шорт нефти риск $1", &TradeIntent::default(), &cl).unwrap().asset, "xyz:CL");
    assert_eq!(IntentParser::apply("лонг золото риск $2", &TradeIntent::default(), &set(&["xyz:GOLD", "ETH"])).unwrap().asset, "xyz:GOLD");
    assert_eq!(IntentParser::apply("лонг TSLA риск $2", &TradeIntent::default(), &set(&["xyz:TSLA", "ETH"])).unwrap().asset, "xyz:TSLA");
    assert_eq!(IntentParser::apply("лонг ETH риск $2", &TradeIntent::default(), &set(&["xyz:ETH", "ETH"])).unwrap().asset, "ETH"); // main exchange first
}

// pending: hyperliquidMarketsFallIntoGroups: Market.group (round 3).
#[test]
fn market_names() {
    assert_eq!(IntentParser::pretty("xyz:TSLA"), "Tesla (xyz:TSLA)");
    assert_eq!(IntentParser::pretty("xyz:CL"), "WTI oil (xyz:CL)");
    assert_eq!(IntentParser::pretty("ETH"), "ETH");
}

/// A price alert tied to a plan prepares it again from `command`: every field must come back as it was.
#[test]
fn plan_command_reads_back_to_the_same_plan() {
    let markets = set(&["SOL", "ETH", "xyz:CL"]);
    let a = TradeIntent { asset: "SOL".into(), risk_usd: Some(d("0.295")), stop_pct: Some(d("3")), take_pct: Some(d("6")), leverage: Some(d("2")), ..TradeIntent::default() };
    let b = TradeIntent {
        asset: "xyz:CL".into(), side: Side::Short, take_r: Some(d("2.5")), auto_rules: Some(true), pullback_pct: Some(d("1.5")), partial_take: Some(true),
        trailing_pct: Some(d("2")), amount_usdc: Some(d("12")), ..TradeIntent::default()
    };
    let c = TradeIntent { asset: "ETH".into(), risk_usd: Some(d("5")), stop_pct: Some(d("2.1")), entry_px: Some(d("2650.5")), leverage: Some(d("3")), ..TradeIntent::default() };
    for plan in [&a, &b, &c] {
        assert_eq!(IntentParser::apply(&plan.command(), &TradeIntent::default(), &markets).as_ref(), Some(plan), "{}", plan.command());
    }
    assert_eq!(a.command(), "long SOL risk $0.295 stop 3% take 6% leverage 2x");
}

/// The plans the executor's tests start from (LimitFillTests, TwapExecutionTests) read as they say.
// pending: the fills and the TWAP run themselves (Execution/Trade).
#[test]
fn executor_plans_parse() {
    let eth = set(&["ETH"]);
    let twap = IntentParser::apply("long ETH 300 USDC over 2 h stop 2% take 4% risk $6 leverage 1x", &TradeIntent::default(), &eth).unwrap();
    assert!(twap.twap_minutes == Some(120) && twap.amount_usdc == Some(d("300")) && twap.take_pct == Some(d("4")) && twap.risk_usd == Some(d("6")));
}

// MARK: Seed guard

const TWELVE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident";

#[test]
fn seed_guard_catches_plain_numbered_and_prefixed() {
    assert!(SeedGuard::contains_mnemonic(TWELVE));
    let numbered: Vec<String> = TWELVE.split(' ').enumerate().map(|(i, w)| format!("{}. {w}", i + 1)).collect();
    assert!(SeedGuard::contains_mnemonic(&numbered.join("\n")));
    assert!(SeedGuard::contains_mnemonic(&format!("вот мои слова: {TWELVE}")));
}

#[test]
fn seed_guard_ignores_ordinary_text() {
    assert!(!SeedGuard::contains_mnemonic("лонг ETH риск $10 стоп 3% тейк 6% плечо 2x"));
    let eleven: Vec<&str> = TWELVE.split(' ').collect();
    assert!(!SeedGuard::contains_mnemonic(&eleven[..11].join(" ")));
}

/// Hardhat's account #0: a public test key.
const HARDHAT_KEY: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

#[test]
fn seed_guard_catches_private_keys() {
    let secret = [7u8; 32];
    let pair: Vec<u8> = secret.iter().copied().chain(wallet::keys::ed25519_public(&secret).unwrap()).collect();
    assert!(SeedGuard::contains_private_key(&format!("import this {}", wallet::keys::base58::encode(&pair))));
    assert!(SeedGuard::contains_private_key(&format!("[{}]", pair.iter().map(u8::to_string).collect::<Vec<_>>().join(", "))));
    assert!(SeedGuard::contains_private_key(&format!("my private key {HARDHAT_KEY}")));
    assert!(SeedGuard::contains_private_key(&format!("вот ключ: 0x{HARDHAT_KEY}")));
}

#[test]
fn seed_guard_lets_signatures_hashes_and_accounts_through() {
    // 64 bytes that aren't a keypair: a Solana transaction's signature.
    let signature: Vec<u8> = (0..64).collect();
    assert!(!SeedGuard::contains_private_key(&format!("what did {} do?", wallet::keys::base58::encode(&signature))));
    // 64 hex digits with no word for a key: a NEAR account, a transaction hash.
    assert!(!SeedGuard::contains_private_key(&format!("send 1 NEAR to {HARDHAT_KEY}")));
    assert!(!SeedGuard::contains_private_key(&format!("what is 0x{HARDHAT_KEY}")));
}

// MARK: Hedge (HedgeTests)

fn hedge_parse(text: &str, to: &TradeIntent) -> Option<TradeIntent> {
    IntentParser::apply(text, to, &set(&["SOL", "ETH", "BTC", "HYPE", "xyz:TSLA", "xyz:NVDA", "xyz:CL"]))
}

fn hedge(text: &str) -> Option<TradeIntent> { hedge_parse(text, &TradeIntent::default()) }

// pending: hedgeFormsParseInEnglishAndRussian: ChatRouter.parses (Normalize round).
#[test]
fn hedge_forms_parse_in_english_and_russian() {
    let all = hedge("hedge SOL").unwrap();
    assert!(all.asset == "SOL" && all.side == Side::Short && all.hedge_share == Some(Decimal::ONE) && all.hedge.is_none());
    assert_eq!(hedge("hedge SOL half").unwrap().hedge_share, Some(d("0.5")));
    assert!(hedge("hedge 0.1 SOL").unwrap().hedge == Some(d("0.1")) && hedge("hedge 0.1 SOL").unwrap().hedge_share.is_none());
    assert_eq!(hedge("hedge ETH leverage 3x").unwrap().leverage, Some(d("3")));
    assert_eq!(hedge("захеджируй SOL").unwrap().hedge_share, Some(Decimal::ONE));
    assert!(hedge("захеджируй половину эфира").is_some_and(|i| i.asset == "ETH" && i.hedge_share == Some(d("0.5"))));
    assert_eq!(hedge("хедж 0,1 SOL").unwrap().hedge, Some(d("0.1")));
    assert_eq!(hedge("hedge TSLA").unwrap().asset, "xyz:TSLA");
    // A share in percent, never as coins; a third said as a percent by the model.
    assert!(hedge("hedge SOL 30%").is_some_and(|i| i.hedge_share == Some(d("0.3")) && i.hedge.is_none()));
    assert!(hedge("захеджируй 50% эфира").is_some_and(|i| i.asset == "ETH" && i.hedge_share == Some(d("0.5"))));
    assert!(hedge("hedge SOL stop 30%").is_some_and(|i| i.hedge_share == Some(Decimal::ONE) && i.stop_pct == Some(d("30"))));
    // No coin named: the model picks one from what the wallet holds. Dollars aren't a hedge's size; «don't hedge» isn't one.
    assert!(hedge("хедж мне что-нибудь").is_none() && hedge("hedge something").is_none());
    assert!(hedge("hedge $500 ETH").is_none() && hedge("hedge ETH 500 USDC").is_none() && hedge("захеджируй эфир на 500 баксов").is_none());
    assert!(hedge("не хеджируй SOL").is_none() && hedge("сними хедж с SOL").is_none());
    assert!(hedge("long SOL, no hedge").is_some_and(|i| !i.is_hedge() && i.side == Side::Long && i.asset == "SOL"));
}

#[test]
fn a_hedge_is_a_plan_of_its_own() {
    // A hedge never keeps the canvas plan's risk, take or side; a long after it is a trade again.
    let long = TradeIntent { asset: "ETH".into(), risk_usd: Some(d("1")), stop_pct: Some(d("3")), take_pct: Some(d("6")), ..TradeIntent::default() };
    let h = hedge_parse("hedge SOL", &long).unwrap();
    assert!(h.risk_usd.is_none() && h.take_pct.is_none() && h.stop_pct.is_none() && h.side == Side::Short);
    // A change alone keeps it a hedge; a take, a risk or money said for one are left out, not listed as changes.
    assert!(hedge_parse("leverage 3x", &h).is_some_and(|i| i.is_hedge() && i.leverage == Some(d("3"))));
    assert_eq!(hedge_parse("take 5% risk $1 10 USDC", &h).as_ref(), Some(&h));
    let trade = hedge_parse("long SOL risk $1", &h).unwrap();
    assert!(!trade.is_hedge() && trade.side == Side::Long);
    // Nothing for the rules to fill, and the command reads back to the same plan.
    assert_eq!(h.started().auto_rules, None);
    for text in ["hedge SOL", "hedge SOL half", "hedge SOL 30%", "hedge 0.1 SOL leverage 3x"] {
        let i = hedge(text).unwrap();
        assert_eq!(hedge(&i.command()).as_ref(), Some(&i), "{text} → {}", i.command());
    }
}

// pending: aHedgeIsSizedByTheCoinsHeld: Plan.resolve, Plan.evaluate and RiskRules on the hedge (round 3).

// MARK: TWAP (TwapTests)

fn twap_parse(text: &str, to: &TradeIntent) -> Option<TradeIntent> { IntentParser::apply(text, to, &set(&["ETH", "BTC", "SOL"])) }

fn twap(text: &str) -> Option<TradeIntent> { twap_parse(text, &TradeIntent::default()) }

/// «Купи ETH на $2000 за 2 часа» is rewritten by the model as the TWAP form; the parser reads it, and the Russian words,
/// the same way. «in 2 hours» / «через 2 часа» say when, not how long: never a TWAP.
// pending: twapFormsParseInEnglishAndRussian: ChatRouter.parses and ChatRouter.forms (Normalize round).
#[test]
fn twap_forms_parse_in_english_and_russian() {
    let en = twap("long ETH 2000 USDC over 2 h leverage 1x").unwrap();
    assert!(en.side == Side::Long && en.asset == "ETH" && en.amount_usdc == Some(d("2000")) && en.leverage == Some(d("1")) && en.twap_minutes == Some(120)
        && en.twap_randomize.is_none());
    let ru = twap("лонг ETH 2000 USDC за 2 часа плечо 1").unwrap();
    assert!(ru.twap_minutes == Some(120) && ru.amount_usdc == Some(d("2000")) && ru.leverage == Some(d("1")));
    assert!(twap("long BTC 500 USDC over 30 min randomize leverage 1x").is_some_and(|i| i.twap_minutes == Some(30) && i.twap_randomize == Some(true)));
    assert!(twap("лонг BTC 500 USDC за полчаса рандомно").is_some_and(|i| i.twap_minutes == Some(30) && i.twap_randomize == Some(true)));
    assert_eq!(twap("шорт SOL 300 USDC в течение 90 минут").unwrap().twap_minutes, Some(90));
    assert_eq!(twap("long ETH 200 USDC twap 1 d").unwrap().twap_minutes, Some(1440));
    assert_eq!(twap("лонг ETH 200 USDC за час").unwrap().twap_minutes, Some(60));
    assert_eq!(twap("long ETH 200 USDC over 1,5 h").unwrap().twap_minutes, Some(90));
    for when in ["long ETH 200 USDC in 2 hours", "лонг ETH 200 USDC через 2 часа"] { assert_eq!(twap(when).and_then(|i| i.twap_minutes), None, "{when}") }
    // «random» is no change without a TWAP; on a TWAP plan «randomize» and «one order» change it.
    assert_eq!(twap("long a random coin").and_then(|i| i.twap_randomize), None);
    assert_eq!(twap_parse("randomize", &en).unwrap().twap_randomize, Some(true));
    assert!(twap_parse("одним ордером", &en).is_some_and(|i| i.twap_minutes.is_none() && i.amount_usdc == Some(d("2000"))));
    assert_eq!(twap_parse("over 3 h", &en).unwrap().twap_minutes, Some(180));
    // The command reads back to the same plan.
    for text in ["long ETH 2000 USDC over 2 h leverage 1x", "long BTC 500 USDC over 30 min randomize leverage 1x"] {
        let i = twap(text).unwrap();
        assert_eq!(twap(&i.command()).as_ref(), Some(&i), "{text} → {}", i.command());
    }
    // Sized by its risk too.
    let risked = twap("long ETH risk $4 stop 0.5% over 5 min").unwrap();
    assert!(risked.risk_usd == Some(d("4")) && risked.stop_pct == Some(d("0.5")) && risked.twap_minutes == Some(5) && risked.amount_usdc.is_none());
    assert!(twap("лонг ETH риск $4 стоп 0.5% за 5 минут").is_some_and(|i| i.risk_usd == Some(d("4")) && i.twap_minutes == Some(5)));
    // Stopping it: by its id, found from the words.
    for text in ["cancel the TWAP", "stop the twap", "отмени TWAP", "останови твап"] { assert_eq!(PositionCommand::parse(text), Some(PositionCommand::CancelOrders), "{text}") }
}

// pending: aTwapKeepsTheRulesOfOneOrder: Plan.evaluate, Plan.resolve and Plan.position of the TWAP (round 3); its phrases parse:
#[test]
fn a_twap_keeps_the_rules_of_one_order_phrases() {
    let i = twap("long ETH 300 USDC over 2 h stop 2% risk $6 leverage 1x").unwrap();
    assert!(i.twap_minutes == Some(120) && i.risk_usd == Some(d("6")) && i.stop_pct == Some(d("2")));
    let started = twap("long ETH 300 USDC over 2 h stop 2% leverage 1x").unwrap().started();
    assert_eq!(started.auto_rules, Some(true));
}

// MARK: TradeIntent's words and Fmt (no Swift test calls these directly; the vectors are what Swift printed, probed with
// swiftc on 2026-11-06)

#[test]
fn changes_name_what_moved() {
    let old = TradeIntent { risk_usd: Some(d("10")), stop_pct: Some(d("3")), ..TradeIntent::default() };
    let new = TradeIntent { side: Side::Short, risk_usd: Some(d("12.5")), stop_pct: None, leverage: Some(d("3")), take_r: Some(d("2")), pullback_pct: Some(d("1.5")),
        twap_minutes: Some(90), twap_randomize: Some(true), ..old.clone() };
    assert_eq!(new.changes(&old), [
        "short", "risk $12.50", "stop —", "leverage 3×", "take 2R", "TWAP over 90 min, randomized", "entry on a 1.5% pullback",
    ]);
    let h = TradeIntent { asset: "SOL".into(), hedge_share: Some(d("0.3")), ..TradeIntent::default() };
    assert_eq!(h.changes(&TradeIntent { asset: "SOL".into(), ..TradeIntent::default() }), ["hedge 30% of the SOL held"]);
    assert_eq!(TradeIntent { entry_px: Some(d("1234567")), ..old.clone() }.changes(&old), ["limit entry at 1,234,570"]);
    assert_eq!([TradeIntent::duration(120), TradeIntent::duration(90), TradeIntent::duration(2880)], ["2 h", "90 min", "2 d"]);
}

#[test]
fn fmt_prices_dots_and_fees_are_swifts() {
    let cases = [
        ("2650.5", "2,650.5"), ("1234567", "1,234,570"), ("0.000012345678", "0.0000123457"), ("0", "0"), ("-2.5", "-2.5"), ("123456.75", "123,457"),
        ("1234565", "1,234,560"), ("1234575", "1,234,580"), ("0.1234565", "0.123456"), ("99999.95", "100,000"), ("999999.5", "1,000,000"), ("12.3456789", "12.3457"),
    ];
    for (n, want) in cases { assert_eq!(Fmt::price(d(n)), want, "{n}") }
    assert_eq!(Fmt::dots("worth $1,234.50 · +4.2% and −$3 and 12 SOL"), "worth ••• · +4.2% and ••• and ••• SOL");
    assert_eq!(Fmt::fee_warning(d("1"), d("0.82")).as_deref(), Some("⚠️ fees take ≈ $0.18 (18%): a bigger amount loses less"));
    assert_eq!(Fmt::fee_warning(d("1"), d("0.95")), None);
    assert_eq!(Fmt::pct(d("2.345")), "2.34%");
}

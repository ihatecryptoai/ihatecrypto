//! ParserFindingsTests, ChatFindingsTests and the read-back half of FormsMatrixTests: phrases a review found read wrong,
//! and the normal forms beside them that must keep reading the same.
//! aQuestionOrANegationIsNoYesOrNo, aBareAmountDoesNotSwitchTheLanguage, aChangeInWordsNeverSignsTheCard and
//! yesAndNoAreWholeMessages (ChatWords, ChatLanguage) are in tests/chat_language.rs.

use std::collections::HashSet;

use rust_decimal::Decimal;

use super::super::{d, set};
use crate::foundation::{adding, unix};
use crate::intent::{Fmt, IntentParser, Side, TradeIntent};
use crate::position_command::{PositionCommand as P, Schedule, SpotAsset, WalletCommand as W};

fn n(s: &str) -> Option<Decimal> { Some(d(s)) }

fn sol() -> SpotAsset { SpotAsset::all()[0].clone() }

fn parse(text: &str) -> Option<W> { W::parse(text) }

fn apply(text: &str) -> Option<TradeIntent> { IntentParser::apply(text, &TradeIntent::default(), &HashSet::new()) }

fn sell_stock(symbol: &str, amount: Option<Decimal>) -> Option<W> { Some(W::SellStock { symbol: symbol.into(), amount }) }

fn sell(asset: SpotAsset, amount: Option<Decimal>) -> Option<W> { Some(W::Sell { asset, amount }) }

// ParserFindingsTests: phrases a review found read wrong (C2–C13, M4, R1–R9), and the normal forms beside them that must
// keep reading the same.

#[test]
fn thousands_and_leading_dots_read_as_written() {   // C4
    assert_eq!(P::parse("move stop to 2,500"), Some(P::StopTo(d("2500"))));
    assert_eq!(P::parse("stop at 2500"), Some(P::StopTo(d("2500"))));
    assert_eq!(P::parse("take profit at 3,100.5"), Some(P::TakeTo(d("3100.5"))));
    assert_eq!(P::parse("закрой 33,333%"), Some(P::Close(d("0.33333"))));
    assert_eq!(parse("send 1,500 USDC to Mom"), Some(W::SendTo { name: "Mom".into(), amount: d("1500"), network: None }));
    assert_eq!(parse("move 1,234.56 USDC from Base to Solana"), Some(W::BaseToSolana(n("1234.56"))));
    assert_eq!(parse("deposit .5 USDC to kamino"), Some(W::Kamino { deposit: true, symbol: "USDC".into(), amount: n("0.5") }));
    let plan = apply("long ETH 1,000 USDC limit 2,650");
    assert!(plan.as_ref().and_then(|p| p.amount_usdc) == n("1000") && plan.as_ref().and_then(|p| p.entry_px) == n("2650"));
    assert_eq!(apply("long ETH risk $1,000").and_then(|p| p.risk_usd), n("1000"));
    // A decimal comma stays one.
    assert_eq!(parse("deposit 1,5 USDC to kamino"), Some(W::Kamino { deposit: true, symbol: "USDC".into(), amount: n("1.5") }));
    assert_eq!(parse("положи 0,500 SOL в хранилище"), Some(W::LendDeposit("SOL".into(), n("0.5"))));
    assert_eq!(parse("buy SOL for 5 USDC"), Some(W::Buy { asset: sol(), usdc: n("5") }));
}

#[test]
fn sol_on_solana_to_a_name_is_a_send_not_a_withdrawal() {   // C2
    let send_sol = |name: &str, amount: &str| Some(W::SendSol { name: name.into(), amount: d(amount) });
    assert_eq!(parse("send 0.5 SOL on solana to New wallet"), send_sol("New wallet", "0.5"));
    assert_eq!(parse("send 0.5 SOL on solana to Bybit"), send_sol("Bybit", "0.5"));
    assert_eq!(parse("отправь 0.5 SOL в солане на Bybit"), send_sol("Bybit", "0.5"));
    assert_eq!(parse("send 0.01 SOL to Bybit"), send_sol("Bybit", "0.01"));
    // The catch-all wants a word for taking money out, or Solana as where it goes.
    assert_eq!(parse("send SOL on solana to New wallet"), None);
    assert_eq!(parse("send 3 USDC to Solana"), Some(W::WithdrawToSolana(n("3"))));
    assert_eq!(parse("withdraw 3 USDC to Solana"), Some(W::WithdrawToSolana(n("3"))));
    assert_eq!(parse("верни всё в солану"), Some(W::WithdrawToSolana(None)));
    assert_eq!(parse("move 2 USDC from Hyperliquid to Solana"), Some(W::WithdrawToSolana(n("2"))));
}

#[test]
fn a_sale_in_dollars_is_never_sell_all() {   // C3
    let m = set(&["BTC", "ETH", "SOL"]);
    for text in ["продай сол на 10 баксов", "sell SOL for 10 USDC", "sell BONK on solana for 10 USDC", "sell DEGEN on base for 10 USDC",
                 "sell TSLA stock for 10 USDC", "sell NEAR for 10 USDC", "sell 0x742d35Cc6634C0532925a3b844Bc454e4438f44e for $5"] {
        assert_eq!(parse(text), None, "{text}");
    }
    // Unparsed all the way: the model is asked again, and no long of 10 USDC is drawn.
    assert_eq!(IntentParser::apply("sell SOL for 10 USDC", &TradeIntent::default(), &m), None);
    // pending: aSaleInDollarsIsNeverSellAll: !ChatRouter.parses("sell SOL for 10 USDC") (Normalize round)
    assert_eq!(parse("sell 2 TSLA stock for 10 USDC"), sell_stock("TSLA", n("2")));
    assert_eq!(parse("sell TSLA stock"), sell_stock("TSLA", None));
    assert_eq!(parse("sell SOL"), sell(sol(), None));
    assert_eq!(parse("sell 0.1 SOL"), sell(sol(), n("0.1")));
    assert_eq!(parse("sell all DEGEN on base"), Some(W::SellOn { place: "base".into(), token: "DEGEN".into(), amount: None, keep: false }));
    assert_eq!(parse("sell 1000 BONK on solana"), sell(SpotAsset::by_symbol("BONK"), n("1000")));
    assert_eq!(IntentParser::apply("short ETH risk $5", &TradeIntent::default(), &m).map(|p| p.side), Some(Side::Short));
}

#[test]
fn a_named_count_is_never_all() {
    let (c, m) = ("0x742d35Cc6634C0532925a3b844Bc454e4438f44e", "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6");
    let m7 = "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr";   // a mint that starts with a digit
    // A bare count is the count, English and Russian: without «tokens» the sale was all of it.
    assert_eq!(parse(&format!("sell 100 {c}")), sell_stock(c, n("100")));
    assert_eq!(parse(&format!("продай 100 {c}")), sell_stock(c, n("100")));
    assert_eq!(parse(&format!("продай 1,5 {c}")), sell_stock(c, n("1.5")));
    assert_eq!(parse(&format!("sell 1,000 {c}")), sell_stock(c, n("1000")));
    assert_eq!(parse(&format!("sell .5 {c}")), sell_stock(c, n("0.5")));
    assert_eq!(parse(&format!("sell my 100 {c}")), sell_stock(c, n("100")));
    assert_eq!(parse(&format!("dump 100 {c}")), sell_stock(c, n("100")));
    assert_eq!(parse(&format!("sell 1500 {m}")), sell(SpotAsset::by_mint(m), n("1500")));
    assert_eq!(parse(&format!("продай 1500 {m}")), sell(SpotAsset::by_mint(m), n("1500")));
    assert_eq!(parse(&format!("sell 3 {m7}")), sell(SpotAsset::by_mint(m7), n("3")));
    assert_eq!(parse("продай 2 теслы"), sell_stock("TSLA", n("2")));
    // The forms the chat writes keep reading the same.
    assert_eq!(parse(&format!("sell 100 tokens {c}")), sell_stock(c, n("100")));
    assert_eq!(parse(&format!("продай 100 штук {c}")), sell_stock(c, n("100")));
    assert_eq!(parse(&format!("sell 1500 tokens {m}")), sell(SpotAsset::by_mint(m), n("1500")));
    // A number or a share it can't place: refused, never «all».
    for t in [
        format!("sell {c} 100"), format!("sell 50% of {c}"), format!("продай половину {c}"), format!("sell half of {m}"), format!("sell 1k {c}"),
        format!("продай {m} 3"), format!("sell {m7} 3"), format!("sell {c} for 10 USDC"), format!("sell $10 of {c}"),
        // Dollars after a space, numbers it would read in part, percents and shares in words, numbers in words.
        format!("sell 100 $ of {c}"), format!("продай 100 $ {c}"), format!("sell 100 $ worth of {m}"), "sell 100 $ of tesla stock".into(),
        format!("продай 1 000 {c}"), format!("продай 1\u{a0}000 {c}"), format!("sell 1 000 tokens {c}"), "sell 1 500 TSLA stock".into(),
        format!("продай 5 тысяч {c}"), format!("sell 1 million {c}"), format!("продай 2 млн {m}"), format!("sell 1/2 of {c}"),
        format!("sell 50 percent of {c}"), format!("продай 50 процентов {c}"), "продай 50 процентов теслы".into(),
        format!("sell a quarter of {c}"), format!("sell a third of {m}"), format!("продай треть {c}"), format!("продай четверть {m}"),
        format!("продай часть {c}"), format!("sell ten {c}"), format!("продай десять {c}"),
    ] {
        assert_eq!(parse(&t), None, "{t}");
    }
    // «All» said, or no number at all: still all.
    assert_eq!(parse(&format!("sell {c}")), sell_stock(c, None));
    assert_eq!(parse(&format!("sell all {c}")), sell_stock(c, None));
    assert_eq!(parse(&format!("продай весь {c}")), sell_stock(c, None));
    assert_eq!(parse(&format!("sell all {m}")), sell(SpotAsset::by_mint(m), None));
    assert_eq!(parse(&format!("sell tokens {m}")), sell(SpotAsset::by_mint(m), None));
    assert_eq!(parse(&format!("sell {m7}")), sell(SpotAsset::by_mint(m7), None));
    assert_eq!(parse("sell TSLA stock"), sell_stock("TSLA", None));
}

#[test]
fn hidden_amounts_are_dots_but_percentages_stay() {
    assert_eq!(Fmt::dots("$1,234.56"), "•••");
    assert_eq!(Fmt::dots("−$1.20 · +4.2%"), "••• · +4.2%");
    assert_eq!(Fmt::dots("0.1583 · gas"), "••• · gas");
    assert_eq!(Fmt::dots("worth $2.39 now · earning 5.97%"), "worth ••• now · earning 5.97%");
    assert_eq!(Fmt::dots("no price"), "no price");
}

#[test]
fn a_buy_in_dollars_is_never_a_long() {
    let m = set(&["BTC", "ETH", "SOL"]);
    let start = TradeIntent::default();
    // The model left out «on solana»: the plan's parser took 750 USDC as a trade's margin and drew a 5× ETH long.
    assert_eq!(IntentParser::apply("buy USDT for 750 USDC", &start, &m), None);
    assert_eq!(IntentParser::apply("купи USDT на 750 баксов", &start, &m), None);
    // pending: aBuyInDollarsIsNeverALong: the two ChatRouter.parses lines (Normalize round)
    assert_eq!(
        parse("send 750 USDT on solana to Bybit USDT").map(|w| w.with_amount(d("749.8"))),
        Some(W::SendToken { name: "Bybit USDT".into(), token: "USDT".into(), amount: n("749.8"), network: Some("solana".into()) })
    );
    // A trade still says so.
    assert_eq!(IntentParser::apply("long ETH 750 USDC", &start, &m).and_then(|p| p.amount_usdc), n("750"));
    // The network's own coin bought or sold for USDC is the swap the forms have for it: Uniswap's list has no ETH.
    let swap = |place: &str, from: &str, to: &str, amount: &str| {
        Some(W::SwapOn { place: place.into(), from: from.into(), to: to.into(), amount: n(amount) })
    };
    assert_eq!(parse("buy ETH on arbitrum for 10 USDC"), swap("arbitrum", "USDC", "ETH", "10"));
    assert_eq!(parse("sell 0.01 ETH on base"), swap("base", "ETH", "USDC", "0.01"));
    assert_eq!(parse("buy POL on polygon for 2 USDC"), swap("polygon", "USDC", "POL", "2"));
    assert_eq!(parse("buy ETH on bnb for 5 USDC"), Some(W::BuyOn { place: "bnb".into(), token: "ETH".into(), usdc: n("5"), from: None }));   // a token there
    assert_eq!(parse("buy ETH on base"), Some(W::BuyOn { place: "base".into(), token: "ETH".into(), usdc: None, from: None }));   // asks how much
    // Jupiter Lend takes USDT both ways, as the brief now says.
    assert_eq!(parse("lend 100 USDT"), Some(W::LendDeposit("USDT".into(), n("100"))));
    assert_eq!(parse("withdraw 100 USDT from the vault"), Some(W::LendWithdraw("USDT".into(), n("100"))));
    // pending: aBuyInDollarsIsNeverALong: the five ChatRouter.planReading lines (Normalize round)
}

#[test]
fn a_question_or_a_dont_never_closes_the_position() {   // C5
    for text in ["не закрывай позицию", "don't close it", "should I close ETH?", "закрыть?", "what if I close half?", "do not move the stop to 2500"] {
        assert_eq!(P::parse(text), None, "{text}");
    }
    assert_eq!(P::parse("закрой половину"), Some(P::Close(d("0.5"))));
    assert_eq!(P::parse("close ETH"), Some(P::Close(d("1"))));
    assert_eq!(P::parse("стоп в безубыток"), Some(P::StopToBreakeven));
    assert_eq!(P::parse("отмени ордер"), Some(P::CancelOrders));
}

#[test]
fn an_answer_in_another_unit_does_not_fill_the_question() {   // C8, R1, R2, R3
    let stake = W::Stake(None);
    let lend = |s: &str| W::LendDeposit(s.into(), None);
    assert_eq!(W::amount_answer_for("$5", &stake), None);
    assert_eq!(W::amount_answer_for("на 5 долларов", &stake), None);
    assert_eq!(W::amount_answer_for("$0.05", &stake), None);
    assert_eq!(W::amount_answer_for("5 usdc", &W::Kamino { deposit: true, symbol: "SOL".into(), amount: None }), None);
    assert_eq!(W::amount_answer_for("1 sol", &lend("USDC")), None);
    assert_eq!(W::amount_answer_for("$5 sol", &stake), None);
    assert_eq!(W::amount_answer_for("0.0000001", &lend("USDC")), None);
    assert_eq!(W::amount_answer_for("0.0000000001", &stake), None);
    assert_eq!(W::amount_answer_for("1,000", &lend("USDC")), n("1000"));
    assert_eq!(W::amount_answer_for("5", &stake), n("5"));
    assert_eq!(W::amount_answer_for("0.5 SOL", &stake), n("0.5"));
    assert_eq!(W::amount_answer_for("0.000000001", &stake), n("0.000000001"));
    assert_eq!(W::amount_answer_for("0.5 сол", &lend("SOL")), n("0.5"));
    assert_eq!(W::amount_answer_for("$5", &W::Buy { asset: sol(), usdc: None }), n("5"));
    assert_eq!(W::amount_answer_for("да, 2", &W::Aave { deposit: true, amount: None, network: "base".into() }), n("2"));
    assert_eq!(W::amount_answer_for("10 usdc", &W::SolanaTo("base".into(), None)), n("10"));
    assert_eq!(W::amount_answer_for("5 баксов", &W::Hlp { deposit: true, amount: None }), n("5"));
    // The old reader, which the app still calls, reads numbers the same way.
    assert_eq!(W::amount_answer("1,5 USDC"), n("1.5"));
    assert_eq!(W::amount_answer("1,000"), n("1000"));
    assert_eq!(W::amount_answer(".5"), n("0.5"));
}

#[test]
fn each_percent_goes_to_its_own_word() {   // C12
    let a = apply("long ETH by the book on a 2% pullback trailing 3%");
    assert!(a.as_ref().and_then(|p| p.pullback_pct) == n("2") && a.as_ref().and_then(|p| p.trailing_pct) == n("3"), "{a:?}");
    let b = apply("long ETH take in parts on a 2% pullback");
    assert!(
        b.as_ref().and_then(|p| p.take_pct).is_none() && b.as_ref().and_then(|p| p.pullback_pct) == n("2") && b.as_ref().and_then(|p| p.partial_take) == Some(true),
        "{b:?}"
    );
    let c = apply("long ETH risk $1 stop 2% pullback 3%");
    assert!(c.as_ref().and_then(|p| p.stop_pct) == n("2") && c.as_ref().and_then(|p| p.pullback_pct) == n("3"), "{c:?}");
    assert_eq!(apply("тейк на 6%").and_then(|p| p.take_pct), n("6"));
    assert_eq!(apply("take profit 6%").and_then(|p| p.take_pct), n("6"));
    assert_eq!(apply("тейк-профит 4%").and_then(|p| p.take_pct), n("4"));
    assert_eq!(apply("вход на откате 2%").and_then(|p| p.pullback_pct), n("2"));
    assert_eq!(apply("pullback of 1.5%").and_then(|p| p.pullback_pct), n("1.5"));
    // The router form reads back to the same plan.
    let mut i = TradeIntent::default();
    i.asset = "SOL".into();
    i.risk_usd = n("1");
    i.stop_pct = n("2");
    i.pullback_pct = n("3");
    i.trailing_pct = n("1");
    i.partial_take = Some(true);
    assert_eq!(IntentParser::apply(&i.command(), &TradeIntent::default(), &set(&["SOL"])), Some(i));
}

#[test]
fn the_network_after_from_is_where_money_leaves() {   // C13
    assert_eq!(parse("переведи 5 USDC с базы на солану"), Some(W::BaseToSolana(n("5"))));
    assert_eq!(parse("переведи 5 USDC с бейза на солану"), Some(W::BaseToSolana(n("5"))));
    assert_eq!(parse("move 5 USDC to Base from Solana"), None);
    assert_eq!(parse("move 5 USDC to Robinhood from Solana"), None);
    assert_eq!(parse("move 5 USDC to Arbitrum from Solana"), None);
    assert_eq!(parse("move 5 USDC from Base to Solana"), Some(W::BaseToSolana(n("5"))));
    assert_eq!(parse("move 5 USDC to Solana from Base"), Some(W::BaseToSolana(n("5"))));
    assert_eq!(parse("move 5 USDC from Solana to Base"), Some(W::SolanaTo("base".into(), n("5"))));
    assert_eq!(parse("верни USDG с робингуда на солану"), Some(W::RobinhoodToSolana(None)));
}

#[test]
fn every_zero_days_is_no_schedule() {   // M4
    assert_eq!(parse("buy SOL for 5 USDC every 0 days"), None);
    assert_eq!(parse("покупай SOL на 5 USDC каждые 0 дней"), None);
    // pending: everyZeroDaysIsNoSchedule: !ChatRouter.parses("buy SOL for 5 USDC every 0 days") (Normalize round)
    assert_eq!(parse("buy SOL for 5 USDC every 3 days"), Some(W::Schedule { asset: sol(), usdc: d("5"), days: 3 }));
    // One stored before the parser refused it moves a day at a time instead of spinning forever.
    let start = unix(1_800_000_000.0);
    let mut s = Schedule::new("SOL", d("5"), 0, start);
    s.advance(start);
    assert_eq!(s.next, adding(start, 86_400.0));
}

/// A recurring buy's period is read whole — weeks and months with their count, a month as 30 days like Jupiter's DCA —
/// and what it doesn't know is refused, never a buy every week.
#[test]
fn recurring_buy_periods() {
    for (text, days) in [
        ("buy SOL for 10 USDC every month", 30), ("buy SOL for 10 USDC each month", 30), ("buy SOL for 10 USDC every 1 month", 30),
        ("buy SOL for 10 USDC monthly", 30), ("buy SOL for 10 USDC every 2 weeks", 14), ("buy SOL for 10 USDC every two weeks", 14),
        ("buy SOL for 10 USDC every other week", 14), ("buy SOL for 10 USDC biweekly", 14), ("buy SOL for 10 USDC every 2 months", 60),
        ("buy SOL for 10 USDC every other month", 60), ("buy SOL for 10 USDC every six months", 180), ("buy SOL for 10 USDC every quarter", 90),
        ("buy SOL for 10 USDC quarterly", 90), ("buy SOL for 10 USDC every week for 2 months", 7), ("buy SOL for 10 USDC every week", 7),
        ("buy SOL for 10 USDC every day", 1), ("buy SOL for 10 USDC every 3 days", 3), ("buy SOL for 10 USDC every monday", 7),
        ("покупай SOL на 10 USDC каждые 2 недели", 14), ("покупай SOL на 10 USDC каждые две недели", 14),
        ("покупай SOL на 10 USDC раз в 2 недели", 14), ("покупай SOL на 10 USDC каждую вторую неделю", 14),
        ("покупай SOL на 10 USDC каждый месяц", 30), ("покупай SOL на 10 USDC раз в месяц", 30), ("покупай SOL на 10 USDC ежемесячно", 30),
        ("купи SOL на 10 USDC ежемесячно", 30), ("купи SOL на 10 USDC еженедельно", 7), ("покупай SOL на 10 USDC каждые 2 месяца", 60),
        ("купи ETH на 10 USDC раз в два месяца", 60), ("покупай SOL на 10 USDC раз в три месяца", 90),
        ("покупай SOL на 10 USDC каждый второй месяц", 60), ("покупай SOL на 10 USDC раз в полгода", 180),
        ("покупай SOL на 10 USDC каждые 3 дня", 3), ("покупай SOL на 10 USDC каждый день", 1), ("покупай SOL на 10 USDC каждую неделю", 7),
        ("покупай SOL на 10 USDC каждую неделю в течение 2 месяцев", 7), ("покупай SOL на 10 USDC каждый понедельник", 7),
        ("покупай SOL на 10 USDC частями", 7),
        // The window's own Russian labels, a day only as the period, years and half-years, «каждое …», «once a …».
        ("покупай SOL на 10 USDC раз в 2 нед.", 14), ("покупай SOL на 10 USDC раз в 3 мес.", 90), ("покупай SOL на 10 USDC раз в день", 1),
        ("покупай SOL на 10 USDC каждые сутки", 1), ("покупай SOL на 10 USDC в первый день каждого месяца", 30),
        ("покупай SOL на 10 USDC каждый месяц в первый день", 30), ("покупай SOL на 10 USDC каждое 1-е число", 30),
        ("buy SOL for 10 USDC on the first day of every month", 30), ("buy SOL for 10 USDC every half year", 180),
        ("buy SOL for 10 USDC semi-annually", 180), ("buy SOL for 10 USDC every year", 365), ("buy SOL for 10 USDC every 2 years", 730),
        ("покупай SOL на 10 USDC раз в 2 года", 730), ("купи SOL на 10 USDC каждое воскресенье", 7), ("buy SOL for 10 USDC once a month", 30),
        ("buy SOL for 10 USDC once a day", 1), ("покупай SOL на $10 раз в месяц", 30), ("покупай SOL на $10 раз в неделю", 7),
        ("покупай SOL на 10 USDC каждый месяц в день зарплаты", 30), ("покупай SOL на 10 USDC раз в месяц в день зарплаты", 30),
        ("buy SOL for 10 USDC every week, starting in a day", 7), ("покупай SOL на 10 USDC каждый рабочий день", 1),
        ("покупай SOL на 10 USDC каждый божий день", 1), ("buy SOL for 10 USDC per day", 1), ("buy SOL for 10 USDC annually", 365),
    ] {
        let asset = if text.contains("ETH") { SpotAsset::all()[2].clone() } else { sol() };
        assert_eq!(parse(text), Some(W::Schedule { asset, usdc: d("10"), days }), "{text}");
    }
    // Hours, minutes and «every 0 …» are refused: never a week, never a one-off buy.
    for text in ["buy SOL for 10 USDC every 4 hours", "buy SOL for 10 USDC every hour", "buy SOL for 10 USDC hourly",
                 "buy SOL for 10 USDC every 30 minutes", "покупай SOL на 10 USDC каждый час", "покупай SOL на 10 USDC каждые 4 часа",
                 "buy SOL for 10 USDC every 0 weeks", "покупай SOL на 10 USDC каждые 0 месяцев", "buy SOL for 10 USDC every 12h",
                 "buy SOL for 10 USDC every 2 h", "buy SOL for 10 USDC every 30 seconds", "buy SOL for 10 USDC twice a week",
                 "покупай SOL на 10 USDC 2 раза в день", "покупай SOL на 10 USDC два раза в неделю",
                 // A count past ten years is refused, not an overflow.
                 "buy SOL for 10 USDC every 999999999999999999 months", "buy SOL for 10 USDC every 400000000000000 days",
                 "buy SOL for 10 USDC every 11 years", "buy SOL for 10 USDC 3 times a week"] {
        assert_eq!(parse(text), None, "{text}");
    }
    // Stopping is never a new schedule, and a word that only sounds periodic doesn't make one.
    for text in ["отмени ежемесячные покупки SOL на 10 USDC", "останови еженедельную покупку SOL на 10 USDC", "stop monthly buys"] {
        assert_eq!(parse(text), Some(W::StopSchedules), "{text}");
    }
    for text in ["купи SOL на 10 USDC, ежедневный лимит 50 USDC", "buy SOL for 10 USDC, what's the annual return",
                 "купи SOL на 10 USDC на каждом кошельке", "buy SOL for 10 USDC once a dip comes"] {
        assert!(!matches!(parse(text), Some(W::Schedule { .. })), "{text} became a schedule");
    }
    // The card's command is the summary: it reads back as the same schedule, its period in weeks or months when whole.
    for days in [1, 2, 3, 7, 10, 14, 21, 28, 30, 45, 60, 90, 180, 210, 365] {
        let c = W::Schedule { asset: sol(), usdc: d("10"), days };
        assert_eq!(parse(&c.summary()), Some(c.clone()), "{}", c.summary());
    }
    assert_eq!(W::Schedule { asset: sol(), usdc: d("10"), days: 14 }.summary(), "buy SOL for 10 USDC every 2 weeks");
    assert_eq!(W::Schedule { asset: sol(), usdc: d("10"), days: 30 }.summary(), "buy SOL for 10 USDC every month");
}

/// «Продай 2 акции теслы» sells 2: the form the model writes has a place for the count, and no count is all of it.
#[test]
fn a_stock_sale_keeps_its_count() {
    // pending: aStockSaleKeepsItsCount: ChatRouter.forms.contains("sell [N] <TICKER> stock") (Normalize round)
    assert_eq!(parse("sell 2 TSLA stock"), sell_stock("TSLA", n("2")));
    assert_eq!(parse("sell 0.5 SPCX stock"), sell_stock("SPCX", n("0.5")));
    assert_eq!(parse("sell 2 Tesla shares"), sell_stock("TSLA", n("2")));
    assert_eq!(parse("продай 2 акции теслы"), sell_stock("TSLA", n("2")));
    assert_eq!(parse("продай 0,5 акции SpaceX"), sell_stock("SPCX", n("0.5")));
    assert_eq!(parse("sell TSLA stock"), sell_stock("TSLA", None));
    assert_eq!(parse("продай все акции теслы"), sell_stock("TSLA", None));
    // A sale in dollars has no form: never read as that many tokens, which the cap at what's held made all of them.
    for t in ["sell $50 of TSLA stock", "sell $50 worth of TSLA stock", "sell 50 worth of Tesla shares", "продай акции теслы на $50",
              "продай на 50 долларов акций теслы", "sell 100 worth of 0x742d35Cc6634C0532925a3b844Bc454e4438f44e"] {
        assert_eq!(parse(t), None, "{t}");
    }
}

// ChatFindingsTests: the chat's safety rules a review found too loose (C1, C10).

// pending: lookupAddressesOnlyInStepsThatShow: ChatRouter.addressesAllowed (Normalize round)

/// The chat asks «How much SOL?» again (naming `amount_asset`) when the reply is an amount, but not one for this question.
#[test]
fn an_amount_in_the_wrong_unit_is_asked_again() {   // C8, R1, R3
    let deposit = W::LendDeposit("USDC".into(), None);
    for (reply, waiting) in [("$5", W::Stake(None)), ("на 5 долларов", W::Stake(None)), ("1 sol", deposit.clone()), ("0.0000001", deposit.clone())] {
        assert!(W::amount_answer(reply).is_some() && W::amount_answer_for(reply, &waiting).is_none(), "{reply}");
    }
    assert!(W::Stake(None).amount_asset() == "SOL" && deposit.amount_asset() == "USDC");
    assert_eq!(W::amount_answer_for("0.5", &W::Stake(None)), n("0.5"));
    assert_eq!(W::amount_answer("long ETH"), None);   // not an amount: the message goes on as before
}

// FormsMatrixTests: every English form the router tells the model to write.

// pending: everyFormReadsAsItSays: the whole matrix through read(...), runCanonical's order (Normalize round)
// pending: everyFormParsesForTheRouter: ChatRouter.parses on every form (Normalize round)

/// Hyperliquid's markets on 2026-10-04, main exchange and xyz, as the app loads them.
fn markets() -> HashSet<String> {
    concat!(
        "BTC,ETH,ATOM,DYDX,SOL,AVAX,BNB,APE,OP,LTC,ARB,DOGE,INJ,SUI,kPEPE,CRV,LDO,LINK,STX,CFX,GMX,SNX,XRP,BCH,APT,AAVE,COMP,WLD,YGG,TRX,kSHIB,UNI,SEI,RUNE,ZRO,DOT,BANANA,TRB,BIGTIME,KAS,BLUR,TIA,BSV,ADA,MINA,POLYX,GAS,PENDLE,FET,NEAR,MEME,ORDI,NEO,ZEN,FIL,PYTH,SUSHI,IMX,kBONK,GMT,SUPER,JUP,kLUNC,RSR,GALA,JTO,ACE,WIF,CAKE,PEOPLE,ENS,ETC,XAI,MANTA,UMA,ONDO,ALT,ZETA,DYM,W,STRK,TAO,AR,kFLOKI,BOME,ETHFI,ENA,MNT,TNSR,SAGA,MERL,HBAR,POPCAT,EIGEN,REZ,NOT,TURBO,BRETT,IO,ZK,RENDER,POL,CELO,HMSTR,kNEIRO,GOAT,MOODENG,GRASS,PURR,PNUT,XLM,SAND,IOTA,ALGO,HYPE,ME,MOVE,VIRTUAL,PENGU,USUAL,FARTCOIN,AIXBT,BIO,GRIFFAIN,SPX,S,MORPHO,TRUMP,MELANIA,ANIME,VVV,BERA,LAYER,KAITO,NIL,PAXG,BABY,WCT,HYPER,ZORA,INIT,NXPC,SOPH,RESOLV,SYRUP,PUMP,PROVE,XPL,WLFI,LINEA,SKY,ASTER,AVNT,STBL,0G,HEMI,APEX,2Z,ZEC,MON,MET,MEGA,CC,ICP,AERO,STABLE,FOGO,LIT,XMR,AXS,DASH,SKR,AZTEC,CHIP,GRAM,CASHCAT,PONS,USELESS,",
        "xyz:XYZ100,xyz:TSLA,xyz:NVDA,xyz:GOLD,xyz:HOOD,xyz:INTC,xyz:PLTR,xyz:COIN,xyz:META,xyz:AAPL,xyz:MSFT,xyz:ORCL,xyz:GOOGL,xyz:AMZN,xyz:AMD,xyz:MU,xyz:SNDK,xyz:MSTR,xyz:CRCL,xyz:NFLX,xyz:COST,xyz:LLY,xyz:SKHX,xyz:TSM,xyz:JPY,xyz:EUR,xyz:SILVER,xyz:RIVN,xyz:BABA,xyz:CL,xyz:COPPER,xyz:NATGAS,xyz:SMSN,xyz:PLATINUM,xyz:USAR,xyz:CRWV,xyz:URNM,xyz:PALLADIUM,xyz:GME,xyz:KR200,xyz:SOFTBANK,xyz:JP225,xyz:HYUNDAI,xyz:KIOXIA,xyz:EWY,xyz:EWJ,xyz:BRENTOIL,xyz:HIMS,xyz:SP500,xyz:DKNG,xyz:LITE,xyz:XLE,xyz:BX,xyz:PURRDAT,xyz:MRVL,xyz:RKLB,xyz:BIRD,xyz:DRAM,xyz:CBRS,xyz:EWZ,xyz:ZM,xyz:EBAY,xyz:ARM,xyz:EWT,xyz:GBP,xyz:SPCX,xyz:ASML,xyz:MINIMAX,xyz:BB,xyz:QNT,xyz:DELL,xyz:IBM,xyz:AVGO,xyz:NOW,xyz:NBIS,xyz:WDC,xyz:NOK,xyz:SMH,xyz:BE,xyz:ZHIPU,xyz:QCOM,xyz:STRC,xyz:BOT,xyz:AMAT,xyz:GIGADEV,xyz:SHAZ,xyz:SKHY,xyz:KSTR,xyz:CXMT,xyz:GEV,xyz:KORU,xyz:UNITREE,xyz:LYTE,xyz:NCLD,xyz:SOXL,xyz:MAGS,xyz:IREN,xyz:NET,xyz:CRWD,xyz:RDDT,xyz:AAOI,xyz:MRNA,xyz:XBI,xyz:SHEIN,xyz:BMNR,xyz:SNXX,xyz:CVX,xyz:TLT,xyz:HO,xyz:OURA",
    )
    .split(',')
    .map(String::from)
    .collect()
}

/// SplitMix64: the same «random» plans on every run.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn chance(&mut self, p: f64) -> bool { (self.next() % 10_000) as f64 / 10_000.0 < p }
    fn pick<T: Clone>(&mut self, a: &[T]) -> T { a[(self.next() % a.len() as u64) as usize].clone() }
}

/// The plans of formCases' «plan» family (random combinations of every optional part, in the form's order), as each
/// must read; the same draws as Swift's, without the command texts.
fn plan_cases() -> Vec<TradeIntent> {
    let mut rng = Seeded(20_261_004);
    let plan_assets = ["BTC", "ETH", "SOL", "HYPE", "xyz:CL", "xyz:BRENTOIL", "xyz:GOLD", "xyz:SILVER", "xyz:TSLA", "xyz:NVDA", "kPEPE", "kBONK",
                       "kPEPE", "AAVE", "LINK", "NEAR", "MOVE", "FARTCOIN", "xyz:SPCX", "xyz:SP500", "ETH", "BTC", "2Z", "0G", "xyz:XYZ100",
                       "xyz:BRENTOIL", "xyz:CL", "xyz:NATGAS", "xyz:COPPER", "xyz:JPY", "ME", "xyz:NOW", "xyz:HOOD", "xyz:MSTR", "VIRTUAL", "PUMP"];
    let money = ["5", "2.5", "10", "1000", "50", "0.5"];
    let risks = ["0.5", "1", "2.5", "10", "0.25"];
    let pcts = ["1", "2.5", "0.8", "3", "6.5", "12"];
    let rs = ["2", "1.5", "3"];
    let levs = ["2", "3", "10", "2.5", "20"];
    let prices = ["2650", "0.1234", "95000", "2650", "67.5", "117250.5"];
    let mut out = Vec::new();
    for _ in 0..3_000 {
        let long = rng.chance(0.5);
        let coin = rng.pick(&plan_assets);
        let mut e = TradeIntent::default();
        e.asset = coin.into();
        e.side = if long { Side::Long } else { Side::Short };
        if rng.chance(0.5) { let v = rng.pick(&money); e.amount_usdc = n(v) }
        if rng.chance(0.5) { let v = rng.pick(&risks); e.risk_usd = n(v) }
        if rng.chance(0.5) { let v = rng.pick(&pcts); e.stop_pct = n(v) }
        match rng.next() % 3 {
            1 => { let v = rng.pick(&pcts); e.take_pct = n(v) }
            2 => { let v = rng.pick(&rs); e.take_r = n(v) }
            _ => {}
        }
        if rng.chance(0.5) { let v = rng.pick(&levs); e.leverage = n(v) }
        if rng.chance(0.3) { e.auto_rules = Some(true) }
        match rng.next() % 3 {
            1 => { let v = rng.pick(&pcts); e.pullback_pct = n(v) }
            2 => { let v = rng.pick(&prices); e.entry_px = n(v) }
            _ => {}
        }
        if rng.chance(0.3) { e.partial_take = Some(true) }
        if rng.chance(0.3) { let v = rng.pick(&pcts); e.trailing_pct = n(v) }
        out.push(e.started());
    }
    out
}

/// The «send token» family's token transfers, as formCases expects them read.
fn send_token_cases() -> Vec<W> {
    let contract = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18";
    let mint = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
    let tokens: [(&str, &[&str]); 12] = [("RAY", &["solana"]), ("AI", &["robinhood"]), ("gtUSDCp", &["base", "arbitrum"]), ("aBasUSDC", &["base"]),
                                         ("jlUSDC", &["solana"]), (contract, &["robinhood", "base"]), (mint, &["solana"]), ("USDG", &["robinhood"]),
                                         ("ETH", &["base", "ethereum"]), ("POL", &["polygon"]), ("SOL", &["solana"]), ("PEPE", &["ethereum"])];
    let mut out = Vec::new();
    for name in ["Mom", "Bybit", "New wallet", "Cold wallet 2"] {
        for (token, nets) in tokens {
            for net in nets {
                let send = |amount| W::SendToken { name: name.into(), token: token.into(), amount, network: Some(net.to_string()) };
                out.push(send(None));
                // 3.5 and 1973.719299 SOL are a SendSol, USDG a SendTo: not token transfers.
                if token != "SOL" && token != "USDG" { out.extend([send(n("3.5")), send(n("1973.719299"))]) }
            }
        }
    }
    out
}

/// A plan written back in its own form (`TradeIntent::command`, what a price alert tied to a plan runs) reads back to the
/// same plan; a token transfer's summary reads back to the same transfer.
/// Every 25th plan by default, as Swift samples its matrix (a debug build reads all 3,000 in ≈ 9 s); `FORMS_MATRIX=full`
/// reads them all.
#[test]
fn forms_read_back_to_what_wrote_them() {
    let markets = markets();
    let full = std::env::var("FORMS_MATRIX").is_ok_and(|v| v == "full");
    let mut wrong = Vec::new();
    for (_, p) in plan_cases().into_iter().enumerate().filter(|(i, _)| full || i % 25 == 0) {
        let back = IntentParser::apply(&p.command(), &TradeIntent::default(), &markets).map(|i| i.started());
        if back.as_ref() != Some(&p) { wrong.push(format!("«{}» → {}", p.command(), back.map_or("nothing".into(), |b| b.command()))) }
    }
    for w in send_token_cases() {
        if parse(&w.summary()) != Some(w.clone()) { wrong.push(format!("«{}» → {:?}", w.summary(), parse(&w.summary()))) }
    }
    assert!(wrong.is_empty(), "read back wrong {}: {:?}", wrong.len(), &wrong[..wrong.len().min(10)]);
}

use chrono::Utc;

use crate::foundation::{Date, adding, unix};
use crate::position_command::WalletCommand;
use crate::scheduled_task::{Every, ScheduledTask, calendar};

/// A time on this computer's clock, as the Swift tests build them (Gregorian, the current time zone).
fn date(day: i64, hour: u32, minute: u32, month: i64) -> Date { calendar::date(2026, month, day, hour, minute).unwrap() }

fn now() -> Date { date(29, 10, 30, 9) }

fn parse(text: &str) -> Option<ScheduledTask> { ScheduledTask::parse(text, now()) }

#[test]
fn scheduled_task_reads_the_router_forms() {
    let week = parse("in 1 week: rebalance USDC").unwrap();
    assert!(week.steps == ["rebalance USDC"] && week.every.is_none() && week.next == date(6, 10, 30, 10));
    assert_eq!(parse("in 30 minutes: lock").unwrap().next, adding(now(), 1800.0));
    // «at 09:00» already passed today: tomorrow; the colon after the time ends the head.
    let nine = parse("at 09:00: what's moving").unwrap();
    assert!(nine.next == date(30, 9, 0, 9) && nine.steps == ["what's moving"]);
    assert_eq!(parse("at 18:15: chart ETH").unwrap().next, date(29, 18, 15, 9));
    assert_eq!(parse("tomorrow at 8:05: profit and loss today").unwrap().next, date(30, 8, 5, 9));
    assert_eq!(parse("on 2026-10-05 at 14:00: withdraw from HLP").unwrap().next, date(5, 14, 0, 10));
    assert_eq!(parse("on 2026-10-05: withdraw from HLP").unwrap().next, date(5, 9, 0, 10));
    assert!(parse("on 2026-09-01: withdraw from HLP").is_none());   // in the past
    // Repeats, and several steps.
    let daily = parse("every day at 09:00: sell SOL then lend USDC").unwrap();
    assert!(daily.every == Some(Every::Days(1)) && daily.next == date(30, 9, 0, 9) && daily.steps == ["sell SOL", "lend USDC"]);
    assert_eq!(parse("every week: rebalance USDC max 40% per protocol").unwrap().every, Some(Every::Days(7)));
    assert_eq!(parse("every 2 weeks: rebalance USDC").unwrap().every.unwrap().words(), "2 weeks");
    assert_eq!(parse("every month: profit and loss month").unwrap().next, date(29, 10, 30, 10));
    assert_eq!(parse("every 6 hours: what's moving").unwrap().next, date(29, 16, 30, 9));
    // 2026-09-29 is a Tuesday: the next Monday is Oct 5.
    let monday = parse("every monday at 10:00: profit and loss week").unwrap();
    assert!(monday.next == date(5, 10, 0, 10) && monday.every == Some(Every::Days(7)));
    assert_eq!(parse("every tuesday: rebalance USDC").unwrap().next, date(6, 9, 0, 10));   // today's 09:00 passed
    // Not tasks.
    assert!(parse("rebalance USDC").is_none());
    assert!(parse("buy SOL for 10 USDC every week").is_none());   // recurring buys keep their form
    assert!(parse("alert me when ETH is above 4000").is_none());
    assert!(parse("every day: in 1 week: lock").is_none());   // no task inside a task
    assert!(parse("every 0 days: lock").is_none());
    assert!(ScheduledTask::cancels_all("cancel scheduled tasks") && ScheduledTask::cancels_all("Stop all scheduled tasks"));
}

#[test]
fn scheduled_task_runs_once_after_a_long_sleep() {
    let mut once = ScheduledTask::new(vec!["lock".into()], now(), None, None);
    assert!(!once.advance(now()));
    let mut daily = ScheduledTask::new(vec!["what's moving".into()], date(20, 9, 0, 9), Some(Every::Days(1)), None);
    assert!(daily.advance(now()) && daily.next == date(30, 9, 0, 9));   // nine missed days: one run, then tomorrow
    let mut monthly = ScheduledTask::new(vec!["profit and loss month".into()], date(31, 9, 0, 8), Some(Every::Months(1)), None);
    assert!(monthly.advance(now()) && monthly.next == date(30, 9, 0, 9));   // Aug 31 → Sep 30, already after now
    let next = monthly.next;
    monthly.advance(next);
    assert_eq!(monthly.next, date(30, 9, 0, 10));   // ponytail: a month-end date drifts to the 30th
}

// pending: the WalletCommand.parse lines of rewardSweepsParseAndScheduleHourly (PositionCommand round).
#[test]
fn reward_sweeps_schedule_hourly() {
    let task = ScheduledTask::parse("every hour: sweep rewards over $10 into USDC", Utc::now()).unwrap();
    assert!(task.every == Some(Every::Hours(1)) && task.steps == ["sweep rewards over $10 into USDC"]);
}

/// The runs a budget has to cover: a daily buy seven times in the next week from an hour from now, a weekly one once,
/// a one-off once inside the window and never after it.
#[test]
fn runs_until_a_date() {
    let now = Utc::now();
    let week = adding(now, 7.0 * 86_400.0 - 1.0);
    let task = |next: Date, every: Option<Every>| ScheduledTask::new(vec!["buy SOL for 5 USDC".into()], next, every, None);
    assert_eq!(task(adding(now, 3600.0), Some(Every::Days(1))).runs(week), 7);
    assert_eq!(task(adding(now, 3600.0), Some(Every::Days(7))).runs(week), 1);
    assert_eq!(task(adding(now, 3600.0), None).runs(week), 1);
    assert_eq!(task(adding(now, 8.0 * 86_400.0), None).runs(week), 0);
    assert_eq!(task(now, Some(Every::Hours(1))).runs(adding(now, 86_400.0)), 25);
}

/// A task's run on the automation wallet pays a service by itself only up to the price the user Confirmed for that
/// request on its card (ScheduledTask.prices): none yet, more, or another request waits for the card. A lower price
/// paid by itself doesn't lower what was Confirmed; the prices are saved with the task, and a task saved before them
/// loads.
#[test]
fn a_task_pays_by_itself_only_up_to_the_confirmed_price() {
    let pay = WalletCommand::Pay { method: "GET".into(), url: "https://a.example/price".into(), body: None };
    let mut task = ScheduledTask::parse("every day at 09:00: pay GET https://a.example/price", Utc::now()).unwrap();
    assert!(!task.approves(60_000, &pay));
    task.approve(60_000, &pay);
    assert!(task.approves(60_000, &pay) && task.approves(50_000, &pay) && !task.approves(60_001, &pay));
    assert!(!task.approves(1, &WalletCommand::Pay { method: "POST".into(), url: "https://a.example/price".into(), body: Some("{}".into()) }));
    assert!(!task.approves(1, &WalletCommand::Pay { method: "GET".into(), url: "https://b.example/price".into(), body: None }));
    task.approve(50_000, &pay);
    assert!(task.approves(60_000, &pay));
    task.approve(1, &WalletCommand::SolanaTo("tempo".into(), Some(2.into())));   // not a paid request: nothing kept
    assert_eq!(task.prices.as_ref().unwrap().len(), 1);
    let back: ScheduledTask = serde_json::from_str(&serde_json::to_string(&task).unwrap()).unwrap();
    assert!(back.approves(60_000, &pay) && !back.approves(60_001, &pay));
    let old = r#"{"id":"6B1E6C8A-3F0B-4C55-9C8E-1D2B3A4F5E6D","steps":["pay GET https://a.example/price"],"next":0}"#;
    let old: ScheduledTask = serde_json::from_str(old).unwrap();
    assert!(!old.approves(1, &pay) && old.next == unix(978_307_200.0));
}

/// What tasks.json holds, as Swift's JSONEncoder writes it.
#[test]
fn a_task_keeps_swifts_shape() {
    let mut task = ScheduledTask::new(vec!["lock".into()], unix(978_307_212.5), Some(Every::Days(7)), None);
    task.paused = Some(true);
    let json = serde_json::to_value(&task).unwrap();
    assert_eq!(json, serde_json::json!({"id": task.id.to_string(), "steps": ["lock"], "next": 12.5, "every": {"days": {"_0": 7}}, "paused": true}));
}

/// A paid request's own text, colons and «then»s inside it, is no task.
#[test]
fn a_pay_request_is_no_task() {
    let pay = r#"pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}"#;
    assert!(ScheduledTask::parse(pay, Utc::now()).is_none());   // pending: the ChatRouter lines of TempoTests (Normalize round)
}

#[test]
fn every_month_on_the_first() {
    let mut task = parse("every month on the 1st at 10:00: send 20 USDC to Mom").unwrap();
    assert!(task.steps == ["send 20 USDC to Mom"] && task.every == Some(Every::Months(1)));
    assert_eq!(task.next, date(1, 10, 0, 10));
    let next = task.next;
    assert!(task.advance(next) && task.next == date(1, 10, 0, 11));
    assert_eq!(parse("every month on day 15: rebalance USDC").unwrap().next, date(15, 9, 0, 10));
    assert!(parse("every month on the 31st: rebalance USDC").is_none());   // not every month has it
}

#[test]
fn when_says_how_often_and_next() {
    let task = parse("every monday at 10:00: profit and loss week").unwrap();
    assert_eq!(task.when(), "every week · next Oct 5 at 10:00\u{202f}AM");
    assert_eq!(parse("on 2026-10-05: withdraw from HLP").unwrap().when(), "once · Oct 5 at 9:00\u{202f}AM");
}

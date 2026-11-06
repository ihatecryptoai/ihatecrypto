//! A command the app runs later, once or on repeat (`ScheduledTask.swift`): «in 1 week: rebalance USDC», «every day at
//! 09:00: sell SOL then lend USDC». Each step is one of the router's English forms and runs as if typed, on a canvas of
//! its own; a money move among them stops at its Confirm card, and the steps after it wait for that move to finish.

use std::collections::HashMap;

use chrono::{Datelike, Local, LocalResult, Months, NaiveDate, NaiveDateTime, TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::agent_commands::AgentCheck;
use crate::foundation::{Date, Uuid, adding, first, group, has, prefix, split, whole};
use crate::intent::Fmt;
use crate::position_command::WalletCommand;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: Uuid,
    pub steps: Vec<String>,
    #[serde(with = "crate::foundation::date")]
    pub next: Date,
    /// nil: once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<Every>,
    /// What started it when the clock didn't: «ETH went below 2,680» for a price alert's steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,
    /// Made through the local debug API, not by the user in the window: it never runs on the automation wallet by
    /// itself, its moves wait for Confirm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untrusted: Option<bool>,
    /// Switched off on the Schedule: it doesn't run until it's switched on again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    /// Pays from the wallet that's on even when there's an automation wallet: its moves wait for Confirm and Touch ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_active: Option<bool>,
    /// The most the user Confirmed on a card for each paid request among its steps («GET https://… body»), in USDC
    /// units: a run on the automation wallet pays one by itself only up to that; a higher price, or none Confirmed yet,
    /// waits for its card (paymentauth §11.11: no unseen price is paid).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<HashMap<String, u64>>,
}

/// How often a task repeats. Written as Swift writes an enum's unnamed values: `{"days":{"_0":7}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "EveryWire", into = "EveryWire")]
pub enum Every {
    Hours(i64),
    Days(i64),
    Months(i64),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EveryWire {
    Hours { _0: i64 },
    Days { _0: i64 },
    Months { _0: i64 },
}

impl From<EveryWire> for Every {
    fn from(w: EveryWire) -> Self { match w { EveryWire::Hours { _0 } => Self::Hours(_0), EveryWire::Days { _0 } => Self::Days(_0), EveryWire::Months { _0 } => Self::Months(_0) } }
}

impl From<Every> for EveryWire {
    fn from(e: Every) -> Self { match e { Every::Hours(n) => Self::Hours { _0: n }, Every::Days(n) => Self::Days { _0: n }, Every::Months(n) => Self::Months { _0: n } } }
}

impl Every {
    pub fn words(self) -> String {
        match self {
            Self::Hours(1) => "hour".into(),
            Self::Hours(n) => format!("{n} hours"),
            Self::Days(1) => "day".into(),
            Self::Days(7) => "week".into(),
            Self::Days(n) if n % 7 == 0 => format!("{} weeks", n / 7),
            Self::Days(n) => format!("{n} days"),
            Self::Months(1) => "month".into(),
            Self::Months(n) => format!("{n} months"),
        }
    }

    fn after(self, d: Date) -> Date {
        match self {
            Self::Hours(n) => adding(d, n as f64 * 3600.0),
            Self::Days(n) => calendar::add_days(d, n).unwrap_or_else(|| adding(d, n as f64 * 86_400.0)),
            Self::Months(n) => calendar::add_months(d, n).unwrap_or_else(|| adding(d, n as f64 * 30.0 * 86_400.0)),
        }
    }
}

impl ScheduledTask {
    pub fn new(steps: Vec<String>, next: Date, every: Option<Every>, because: Option<String>) -> Self {
        Self { id: Uuid::new(), steps, next, every, because, untrusted: None, paused: None, from_active: None, prices: None }
    }

    fn price_key(c: &WalletCommand) -> Option<String> {
        match c {
            WalletCommand::Pay { method, url, body } => Some(format!("{method} {url} {}", body.as_deref().unwrap_or(""))),
            _ => None,
        }
    }

    pub fn approves(&self, units: u64, c: &WalletCommand) -> bool {
        Self::price_key(c).and_then(|k| self.prices.as_ref()?.get(&k).copied()).is_some_and(|max| units <= max)
    }

    /// A price paid in its run: one Confirmed on the card raises what later runs may pay; one paid by itself is never
    /// above it.
    pub fn approve(&mut self, units: u64, c: &WalletCommand) {
        let Some(k) = Self::price_key(c) else { return };
        let prices = self.prices.get_or_insert_with(HashMap::new);
        let kept = prices.entry(k).or_insert(units);
        *kept = (*kept).max(units);
    }

    /// «cancel scheduled tasks» and its wordings: every task goes.
    pub fn cancels_all(text: &str) -> bool {
        has(&text.to_lowercase(), r"(?:cancel|stop|remove|delete|clear)\s+(?:all\s+)?(?:the\s+)?scheduled\s+tasks?")
    }

    /// The router's forms: `in N <minutes|hours|days|weeks|months>: …`, `at HH:MM: …`, `tomorrow [at HH:MM]: …`,
    /// `on YYYY-MM-DD [at HH:MM]: …`, `every <hour|day|week|month|N hours|N days|N weeks|N months> [at HH:MM]: …`,
    /// `every month on the <1st…28th> [at HH:MM]: …`,
    /// `every <monday…sunday> [at HH:MM]: …` (09:00 when no time is said),
    /// steps joined by « then ». nil for anything else.
    pub fn parse(text: &str, now: Date) -> Option<Self> {
        let colon = text.find(':')?;
        // The colon that ends «… at 09:00:» is the one after the time.
        let colon = if prefix(&text[colon..], r":\d\d:").is_some() { colon + text[colon..].chars().take(3).map(char::len_utf8).sum::<usize>() } else { colon };
        let head = text[..colon].to_lowercase();
        let head = crate::foundation::trim_spaces(&head);
        let rest = text[colon + 1..].trim();
        // A background check (AgentCheck) is one step however it's worded: its «;» and «then» are the model's to read.
        let steps: Vec<String> = if AgentCheck::parse(rest).is_some() {
            vec![rest.into()]
        } else {
            split(rest, r"\s+then\s+|\s*;\s*").into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        };
        // No task inside a task.
        if steps.is_empty() || steps.iter().any(|s| Self::parse(s, Utc::now()).is_some()) { return None }
        let clock = first(head, r"\bat\s+(\d{1,2}):(\d\d)$").and_then(|m| {
            let (h, mi) = (group(&m, 1).parse::<u32>().ok()?, group(&m, 2).parse::<u32>().ok()?);
            (h < 24 && mi < 60).then_some((h, mi))
        });
        // The first time at that clock on or after `day`'s date, and after now.
        let at = |day: Date, (h, mi): (u32, u32)| {
            let d = calendar::set_time(day, h, mi)?;
            if d <= now { calendar::add_days(d, 1) } else { Some(d) }
        };
        let once = |next: Date| Some(Self::new(steps.clone(), next, None, None));
        if let Some(m) = whole(head, r"in\s+(\d+)\s+(minute|hour|day|week|month)s?") {
            let n: i64 = group(&m, 1).parse().ok().filter(|&n| n > 0)?;
            return once(match group(&m, 2) {
                "minute" => adding(now, n as f64 * 60.0),
                "hour" => Every::Hours(n).after(now),
                "day" => Every::Days(n).after(now),
                "week" => Every::Days(7 * n).after(now),
                _ => Every::Months(n).after(now),
            });
        }
        if let Some(c) = clock
            && whole(head, r"at\s+\d{1,2}:\d\d").is_some()
            && let Some(next) = at(now, c)
        {
            return once(next);
        }
        if whole(head, r"tomorrow(?:\s+at\s+\d{1,2}:\d\d)?").is_some() && let Some(tomorrow) = calendar::add_days(now, 1) {
            return once(clock.and_then(|(h, mi)| calendar::set_time(tomorrow, h, mi)).unwrap_or(tomorrow));
        }
        let (hour, minute) = clock.unwrap_or((9, 0));
        if let Some(m) = whole(head, r"on\s+(\d{4})-(\d\d)-(\d\d)(?:\s+at\s+\d{1,2}:\d\d)?")
            && let Some(day) = calendar::date(group(&m, 1).parse().ok()?, group(&m, 2).parse().ok()?, group(&m, 3).parse().ok()?, hour, minute)
            && day > now
        {
            return once(day);
        }
        let weekdays = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
        if let Some(m) = whole(head, r"every\s+(sunday|monday|tuesday|wednesday|thursday|friday|saturday)(?:\s+at\s+\d{1,2}:\d\d)?")
            && let Some(w) = weekdays.iter().position(|d| *d == group(&m, 1))
            && let Some(next) = calendar::next_weekday(now, w as u32, hour, minute)
        {
            return Some(Self::new(steps, next, Some(Every::Days(7)), None));
        }
        // «every month on the 1st at 10:00»: that day of each month, 1–28 so every month has it; 09:00 when no time is said.
        if let Some(m) = whole(head, r"every\s+month\s+on\s+(?:the\s+|day\s+)?(\d{1,2})(?:st|nd|rd|th)?(?:\s+at\s+\d{1,2}:\d\d)?")
            && let Some(day) = group(&m, 1).parse::<u32>().ok().filter(|d| (1..=28).contains(d))
            && let Some(next) = calendar::next_month_day(now, day, hour, minute)
        {
            return Some(Self::new(steps, next, Some(Every::Months(1)), None));
        }
        if let Some(m) = whole(head, r"every\s+(?:(\d+)\s+)?(hour|day|week|month)s?(?:\s+at\s+\d{1,2}:\d\d)?") {
            let n: i64 = group(&m, 1).parse().unwrap_or(1);
            if n <= 0 { return None }
            let every = match group(&m, 2) { "hour" => Every::Hours(n), "day" => Every::Days(n), "week" => Every::Days(7 * n), _ => Every::Months(n) };
            // «every day at 09:00»: the next 09:00; without a time, one period from now.
            let next = clock.and_then(|c| at(now, c)).unwrap_or_else(|| every.after(now));
            return Some(Self::new(steps, next, Some(every), None));
        }
        None
    }

    /// After a run: false when it was the only one; else the next date strictly in the future, so a computer asleep for
    /// a week runs a daily task once, not seven times.
    pub fn advance(&mut self, now: Date) -> bool {
        let Some(every) = self.every else { return false };
        loop {
            self.next = every.after(self.next);
            if self.next > now { return true }
        }
    }

    /// How many times it runs from its next run until `date`: what its buys take from a budget by then.
    pub fn runs(&self, until: Date) -> i64 {
        if self.next > until { return 0 }
        let Some(every) = self.every else { return 1 };
        let (mut n, mut d) = (0, self.next);
        while d <= until && n < 10_000 {
            n += 1;
            d = every.after(d);
        }
        n
    }

    /// «every week · next Oct 6 at 9:00 AM» / «once · Oct 6 at 9:00 AM»
    pub fn when(&self) -> String {
        format!("{}{}", self.every.map_or("once · ".into(), |e| format!("every {} · next ", e.words())), Fmt::date(self.next))
    }
}

/// Dates are Gregorian in this computer's time zone, whatever its calendar shows (the Mac's is set to Buddhist years):
/// Foundation's Calendar as ScheduledTask uses it.
pub(crate) mod calendar {
    use super::*;

    fn local(d: Date) -> NaiveDateTime { d.with_timezone(&Local).naive_local() }

    /// A wall-clock time here: in a daylight-saving gap the hour after it (Foundation moves past the gap), in the
    /// repeated hour the first of the two.
    // ponytail: a gap taken as one hour, as nearly every zone's is; Lord Howe's 30 minutes would land 30 minutes late.
    pub fn instant(n: NaiveDateTime) -> Option<Date> {
        match Local.from_local_datetime(&n) {
            LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => Some(d.with_timezone(&Utc)),
            LocalResult::None => Local.from_local_datetime(&(n + TimeDelta::hours(1))).earliest().map(|d| d.with_timezone(&Utc)),
        }
    }

    /// `date(byAdding: .day, value: n, to: d)`: the same wall-clock time `n` days on.
    pub fn add_days(d: Date, n: i64) -> Option<Date> { instant(local(d).checked_add_signed(TimeDelta::try_days(n)?)?) }

    /// `date(byAdding: .month, …)`: a day the month hasn't becomes its last (Aug 31 + 1 month = Sep 30).
    pub fn add_months(d: Date, n: i64) -> Option<Date> {
        let l = local(d);
        let months = Months::new(u32::try_from(n.unsigned_abs()).ok()?);
        instant(if n >= 0 { l.checked_add_months(months)? } else { l.checked_sub_months(months)? })
    }

    /// `date(bySettingHour:minute:second: 0, of: day)`.
    pub fn set_time(day: Date, hour: u32, minute: u32) -> Option<Date> { instant(local(day).date().and_hms_opt(hour, minute, 0)?) }

    /// `date(from: DateComponents(year:month:day:hour:minute:))`, lenient as Foundation is: Feb 30 is Mar 2, month 13
    /// is next January.
    pub fn date(year: i32, month: i64, day: i64, hour: u32, minute: u32) -> Option<Date> {
        let start = NaiveDate::from_ymd_opt(year, 1, 1)?;
        let first = if month >= 1 { start.checked_add_months(Months::new(u32::try_from(month - 1).ok()?))? }
                    else { start.checked_sub_months(Months::new(u32::try_from(1 - month).ok()?))? };
        instant(first.checked_add_signed(TimeDelta::try_days(day - 1)?)?.and_hms_opt(hour, minute, 0)?)
    }

    /// `nextDate(after: now, matching: weekday + hour + minute, matchingPolicy: .nextTime)`; `weekday` 0 is Sunday.
    pub fn next_weekday(now: Date, weekday: u32, hour: u32, minute: u32) -> Option<Date> {
        let today = local(now).date();
        (0..=7).map(|i| today + TimeDelta::days(i)).filter(|d| d.weekday().num_days_from_sunday() == weekday)
            .filter_map(|d| instant(d.and_hms_opt(hour, minute, 0)?)).find(|&d| d > now)
    }

    /// `nextDate(after: now, matching: day + hour + minute)` for a day every month has (1–28).
    pub fn next_month_day(now: Date, day: u32, hour: u32, minute: u32) -> Option<Date> {
        let l = local(now).date();
        let this = NaiveDate::from_ymd_opt(l.year(), l.month(), day)?;
        (0..=12).filter_map(|i| this.checked_add_months(Months::new(i))).filter_map(|d| instant(d.and_hms_opt(hour, minute, 0)?)).find(|&d| d > now)
    }
}

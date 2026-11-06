use chrono::{Local, TimeZone};
use rust_decimal::Decimal;

use super::d;
use crate::foundation::*;
use crate::intent::Fmt;

/// Swift's `\b` (UAX #29): «base.org», «xyz:gold», «don't», «2,800» and «1.5x» hold no boundary inside; «on base.» and
/// «a.1» do.
#[test]
fn word_boundaries_are_swifts() {
    assert!(!has("base.org", r"\bbase\b") && has("on base.", r"\bbase\b") && !has("xyz:gold", r"\bgold\b"));
    assert!(!has("don't", r"\bdon\b") && !has("2,800", r"\b800\b") && !has("1.5x", r"\b5x") && has("a.1", r"\b1") && !has("U.S", r"\bS"));
    assert!(has("сохрани Vitalik.eth как", r"\bкак\b"));
}

#[test]
fn decimal_from_string_reads_a_leading_number() {
    assert_eq!(decimal("12abc"), Some(d("12")));
    assert_eq!(decimal(" 12"), Some(d("12")));
    assert_eq!(decimal(""), None);
    assert_eq!(decimal("abc"), None);
    assert_eq!(decimal(".5"), Some(d("0.5")));
    assert_eq!(decimal("1e3"), Some(d("1000")));
    assert_eq!(decimal("-"), Some(Decimal::ZERO));
    assert_eq!(decimal("1."), Some(d("1")));
    assert_eq!(decimal("+3"), Some(d("3")));
    assert_eq!(text(d("2.50")), "2.5");
    assert_eq!(text(d("100.000")), "100");
    assert_eq!(text(d("-0")), "0");
}

#[test]
fn rounding_modes_are_nsdecimalrounds() {
    assert_eq!(rounded(d("-1.55"), 1, Rounding::Down), d("-1.6"));
    assert_eq!(rounded(d("-1.55"), 1, Rounding::Plain), d("-1.6"));
    assert_eq!(rounded(d("1.21"), 1, Rounding::Up), d("1.3"));
}

/// en_US through ICU: half to even, grouped thousands, a minus kept on a negative that rounds to zero.
#[test]
fn numbers_print_as_swifts_format_style() {
    assert_eq!(Fmt::num(d("0.00001196"), 8), "0.00001196");
    assert_eq!(Fmt::num(d("25"), 6), "25");
    assert_eq!(Fmt::num(d("1234567.5"), 6), "1,234,567.5");
    assert_eq!(Fmt::num(d("-0.004"), 2), "-0");
    assert_eq!(Fmt::num(d("0.005"), 2), "0");
    assert_eq!(Fmt::num(d("0.015"), 2), "0.02");
    assert_eq!(Fmt::num(d("2.5"), 0), "2");
    assert_eq!(Fmt::num(d("-1234.5"), 2), "-1,234.5");
    assert_eq!(Fmt::num(d("12345678901234567890.5"), 2), "12,345,678,901,234,567,890.5");
    assert_eq!(Fmt::usd(d("0.125")), "$0.12");
    assert_eq!(Fmt::usd(d("0.135")), "$0.14");
    assert_eq!(Fmt::usd(d("-1.5")), "−$1.50");
    assert_eq!(Fmt::usd(d("-0.001")), "$0.00");
}

#[test]
fn dates_print_as_the_macs() {
    let d = Local.with_ymd_and_hms(2026, 9, 21, 17, 13, 0).unwrap().with_timezone(&chrono::Utc);
    assert_eq!(Fmt::date(d), "Sep 21 at 5:13\u{202f}PM");
    let midnight = Local.with_ymd_and_hms(2026, 9, 22, 0, 13, 0).unwrap().with_timezone(&chrono::Utc);
    assert_eq!(Fmt::date(midnight), "Sep 22 at 12:13\u{202f}AM");
}

#[test]
fn capitalized_is_foundations() {
    let got: Vec<String> = ["hello-world", "o'neil", "x.y", "twitter", "hELLO wORLD", "1abc", "a_b", "über", "x:y"].iter().map(|s| capitalized(s)).collect();
    assert_eq!(got, ["Hello-World", "O'neil", "X.Y", "Twitter", "Hello World", "1Abc", "A_B", "Über", "X:Y"]);
}

/// What JSONEncoder wrote for `{id, next: 12.5 s after 2001, every: .hours(3), x: 2003.80, y: 0.1 / 3}`.
#[test]
fn codable_shapes_are_swifts() {
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct T {
        id: Uuid,
        #[serde(with = "date")]
        next: Date,
        every: Option<crate::scheduled_task::Every>,
        #[serde(with = "number")]
        x: Decimal,
        #[serde(with = "number")]
        y: Decimal,
    }
    let swift = r#"{"id":"8E8AF119-6489-4740-895A-68BA953266F6","y":0.033333333333333333333333333333333333333,"x":2003.8,"next":12.5,"every":{"hours":{"_0":3}}}"#;
    let t: T = serde_json::from_str(swift).unwrap();
    assert_eq!(t.id.to_string(), "8E8AF119-6489-4740-895A-68BA953266F6");
    assert_eq!(seconds(t.next), 978_307_212.5);
    assert_eq!(t.every, Some(crate::scheduled_task::Every::Hours(3)));
    assert_eq!(t.x, d("2003.8"));
    // ponytail: Decimal holds 28 digits; Swift's 38 are rounded to them.
    assert_eq!(t.y, d("0.0333333333333333333333333333"));
    let back = serde_json::to_string(&t).unwrap();
    assert_eq!(back, r#"{"id":"8E8AF119-6489-4740-895A-68BA953266F6","next":12.5,"every":{"hours":{"_0":3}},"x":2003.8,"y":0.0333333333333333333333333333}"#);
    // ponytail: read from text every digit stays; through a serde_json::Value (an f64 inside) a long one keeps 17.
    assert_eq!(serde_json::from_str::<T>(&back).unwrap(), t);
    assert!(Uuid::parse("8e8af119-6489-4740-895a-68ba953266f6").is_some() && Uuid::parse("8E8AF119648947408 95A68BA953266F6").is_none());
    assert_eq!(Uuid::new().to_string().as_bytes()[14], b'4');
}

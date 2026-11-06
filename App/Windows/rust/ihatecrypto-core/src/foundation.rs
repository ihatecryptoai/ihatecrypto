//! What Swift and Foundation do underneath the ported code, done the same way: regex literals (with Swift's word
//! boundaries), Characters (grapheme clusters) and their properties, Decimal's parsing, rounding and printing, `Date`
//! and `UUID` as `Codable` writes them.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, TimeDelta, Utc};
use fancy_regex::Regex;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

// MARK: Regex

pub(crate) type Caps<'t> = fancy_regex::Captures<'t, str>;

/// Swift's `\b` is Unicode's default word boundary (UAX #29), not the edge of `\w`: «base.org», «xyz:gold», «don't»
/// and «2,800» are one word each there. A letter–mark–letter or digit–mark–digit run stays one word here too. And it
/// breaks between two characters that aren't a word's and at the text's ends («1.5 × the», «$ 5»), except inside a run
/// of spaces (WB3d): Swift's `/×\b/` finds «× the».
// ponytail: those joins only; UAX #29's other rules (Katakana, emoji, a format character inside a word, CR LF, a
// boundary between two marks) are the `\w` edge's. Add them when a phrase needs one.
const BOUNDARY: &str = concat!(
    r"(?:\b(?!(?<=\p{L})[.:'’·](?=\p{L}))(?!(?<=\d)[.,;'’](?=\d))(?!(?<=\p{L}[.:'’·])\p{L})(?!(?<=\d[.,;'’])\d)",
    r"|(?<!\w)(?!\w)(?!(?<=[\p{Zs}--[\x{A0}\x{2007}\x{202F}]])[\p{Zs}--[\x{A0}\x{2007}\x{202F}]]))",
);

type Cache = Mutex<HashMap<(&'static str, u8), &'static Regex>>;
static CACHE: LazyLock<Cache> = LazyLock::new(Default::default);

/// A regex literal, compiled the first time and kept: `anchor` 1 is `wholeMatch`, 2 `prefixMatch`.
fn compiled(pattern: &'static str, anchor: u8) -> &'static Regex {
    let mut cache = CACHE.lock().expect("the regex cache");
    cache.entry((pattern, anchor)).or_insert_with(|| {
        let p = pattern.replace(r"\b", BOUNDARY);
        let p = match anchor { 1 => format!(r"\A(?:{p})\z"), 2 => format!(r"\A(?:{p})"), _ => p };
        Box::leak(Box::new(Regex::new(&p).unwrap_or_else(|e| panic!("{pattern}: {e}"))))
    })
}

/// `text.contains(/…/)`. A match the engine gives up on (its backtracking limit) is no match.
pub(crate) fn has(text: &str, pattern: &'static str) -> bool { compiled(pattern, 0).is_match(text).unwrap_or(false) }

/// `text.firstMatch(of: /…/)`.
pub(crate) fn first<'t>(text: &'t str, pattern: &'static str) -> Option<Caps<'t>> { compiled(pattern, 0).captures(text).ok().flatten() }

/// `text.wholeMatch(of: /…/)`.
pub(crate) fn whole<'t>(text: &'t str, pattern: &'static str) -> Option<Caps<'t>> { compiled(pattern, 1).captures(text).ok().flatten() }

/// `text.prefixMatch(of: /…/)`.
pub(crate) fn prefix<'t>(text: &'t str, pattern: &'static str) -> Option<Caps<'t>> { compiled(pattern, 2).captures(text).ok().flatten() }

/// `text.matches(of: /…/)`.
pub(crate) fn all<'t>(text: &'t str, pattern: &'static str) -> Vec<Caps<'t>> {
    compiled(pattern, 0).captures_iter(text).map_while(Result::ok).collect()
}

/// `text.split(separator: /…/)`: empty pieces left out.
pub(crate) fn split<'t>(text: &'t str, pattern: &'static str) -> Vec<&'t str> {
    compiled(pattern, 0).split(text).map_while(Result::ok).filter(|s| !s.is_empty()).collect()
}

/// `text.replacing(/…/) { … }`: each match by what `with` makes of it.
pub(crate) fn replacing(text: &str, pattern: &'static str, with: impl FnMut(&Caps<'_>) -> String) -> String {
    compiled(pattern, 0).replace_all(text, with).into_owned()
}

/// A group's text, "" when it took no part.
pub(crate) fn group<'t>(c: &Caps<'t>, i: usize) -> &'t str { c.get(i).map_or("", |m| m.as_str()) }

// MARK: Characters

/// Swift's Characters: grapheme clusters.
pub(crate) fn chars(s: &str) -> Vec<&str> { s.graphemes(true).collect() }

/// `String.count`.
pub(crate) fn count(s: &str) -> usize { s.graphemes(true).count() }

fn head(g: &str) -> char { g.chars().next().unwrap_or('\0') }

/// `Character.isLetter`: its first scalar is alphabetic.
pub(crate) fn is_letter(g: &str) -> bool { head(g).is_alphabetic() }

/// `Character.isNumber`: its first scalar is a number.
// ponytail: Unicode's N* categories; Swift asks for a numeric type, which also takes CJK numerals (一, 二).
pub(crate) fn is_number(g: &str) -> bool { head(g).is_numeric() }

/// `Character.isUppercase`.
pub(crate) fn is_upper(g: &str) -> bool { head(g).is_uppercase() }

/// `Character.isWhitespace`.
pub(crate) fn is_space(g: &str) -> bool { head(g).is_whitespace() }

/// `s.contains(where:)` over Characters.
pub(crate) fn any_char(s: &str, f: impl Fn(&str) -> bool) -> bool { s.graphemes(true).any(f) }

/// `s.allSatisfy` over Characters (true for "").
pub(crate) fn all_chars(s: &str, f: impl Fn(&str) -> bool) -> bool { s.graphemes(true).all(f) }

/// `split(whereSeparator:)`: the runs between separator Characters, empty ones left out.
pub(crate) fn split_chars(s: &str, separator: impl Fn(&str) -> bool) -> Vec<&str> {
    let (mut out, mut start) = (Vec::new(), 0);
    for (i, g) in s.grapheme_indices(true) {
        if separator(g) {
            if i > start { out.push(&s[start..i]) }
            start = i + g.len();
        }
    }
    if start < s.len() { out.push(&s[start..]) }
    out
}

/// `trimmingCharacters(in: .whitespaces)`: spaces and tabs, not line breaks.
pub(crate) fn trim_spaces(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\t' || (c.is_whitespace() && !matches!(c, '\n'..='\r' | '\u{85}' | '\u{2028}' | '\u{2029}')))
}

/// `trimmingCharacters(in: CharacterSet(charactersIn:))`.
pub(crate) fn trim_set<'a>(s: &'a str, set: &str) -> &'a str { s.trim_matches(|c| set.contains(c)) }

/// `String.capitalized`: each word's first letter upper case, the rest lower; an apostrophe doesn't start a word
/// («o'neil» → «O'neil», «x.y» → «X.Y», «1abc» → «1Abc», as Foundation gives them).
pub(crate) fn capitalized(s: &str) -> String {
    let mut out = String::new();
    let mut in_word = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if in_word { out.extend(c.to_lowercase()) } else { out.extend(c.to_uppercase()) }
            in_word = true;
        } else {
            out.push(c);
            in_word = in_word && (c == '\'' || c == '’');
        }
    }
    out
}

// MARK: Decimal

/// `NSDecimalNumber.stringValue`, `Decimal.description` and how `JSONEncoder` writes a Decimal: no exponent, no
/// trailing zeros.
pub fn text(d: Decimal) -> String { if d.is_zero() { "0".into() } else { d.normalize().to_string() } }

/// `Decimal(string:)`: the number at the start of the text, spaces before it skipped and what follows ignored
/// («12abc» is 12); nil when there is none. A sign alone is 0, as in Foundation.
// ponytail: up to Decimal's 28 digits here; Swift keeps 38, so a longer number is nil.
pub fn decimal(s: &str) -> Option<Decimal> {
    let t = s.trim_start();
    let b = t.as_bytes();
    let run = |from: usize| from + b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut i = usize::from(matches!(b.first(), Some(b'-' | b'+')));
    let whole = &t[i..run(i)];
    i = run(i);
    let mut fraction = "";
    if b.get(i) == Some(&b'.') {
        fraction = &t[i + 1..run(i + 1)];
        i = run(i + 1);
    }
    if i == 0 { return None }
    let sign = if b[0] == b'-' { "-" } else { "" };
    let mut number = format!("{sign}{}.{}", if whole.is_empty() { "0" } else { whole }, if fraction.is_empty() { "0" } else { fraction });
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let from = i + 1 + usize::from(matches!(b.get(i + 1), Some(b'-' | b'+')));
        if run(from) > from { number = format!("{number}e{}", &t[i + 1..run(from)]) }
    }
    if number.contains('e') { Decimal::from_scientific(&number).ok() } else { Decimal::from_str(&number).ok() }
}

/// `NSDecimalNumber.RoundingMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Half away from zero.
    Plain,
    /// Toward −∞.
    Down,
    /// Toward +∞.
    Up,
    /// Half to even.
    Bankers,
}

/// `Decimal.rounded(_:_:)` (Intent.swift), NSDecimalRound: to `scale` places.
pub fn rounded(d: Decimal, scale: u32, mode: Rounding) -> Decimal {
    d.round_dp_with_strategy(scale, match mode {
        Rounding::Plain => RoundingStrategy::MidpointAwayFromZero,
        Rounding::Down => RoundingStrategy::ToNegativeInfinity,
        Rounding::Up => RoundingStrategy::ToPositiveInfinity,
        Rounding::Bankers => RoundingStrategy::MidpointNearestEven,
    })
}

/// `pow(10, n)` as a Decimal.
pub(crate) fn pow10(n: u32) -> Decimal { Decimal::from_i128_with_scale(10i128.pow(n), 0) }

/// `Decimal(string:)` of a literal in the code.
pub(crate) fn dec(s: &str) -> Decimal { Decimal::from_str(s).expect("a decimal literal") }

/// A number JSONSerialization read, as `NSNumber.decimalValue` gives it: a whole number as is, a fraction as the
/// shortest decimal that reads back the same double.
pub(crate) fn json_decimal(v: &Value) -> Option<Decimal> {
    let n = v.as_number()?;
    n.as_i64().map(Decimal::from).or_else(|| n.as_u64().map(Decimal::from)).or_else(|| decimal(&format!("{}", n.as_f64()?)))
}

/// A Decimal in `Codable` JSON: a number with every digit (`#[serde(with = "foundation::number")]`).
pub mod number {
    use super::*;
    use serde_json::value::RawValue;

    pub fn serialize<S: Serializer>(d: &Decimal, s: S) -> Result<S::Ok, S::Error> {
        RawValue::from_string(text(*d)).map_err(serde::ser::Error::custom)?.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        let t = raw.get();
        let n = if t.starts_with('"') { None } else { Decimal::from_str(t).or_else(|_| Decimal::from_scientific(t)).ok() };
        n.ok_or_else(|| serde::de::Error::custom(format!("expected a number, found {t}")))
    }

    /// An optional one, left out when nil (`#[serde(default, skip_serializing_if = "Option::is_none", with = …)]`).
    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(d: &Option<Decimal>, s: S) -> Result<S::Ok, S::Error> {
            match d { Some(d) => super::serialize(d, s), None => s.serialize_none() }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Decimal>, D::Error> {
            #[derive(Deserialize)]
            struct Wrap(#[serde(with = "super")] Decimal);
            Ok(Option::<Wrap>::deserialize(d)?.map(|w| w.0))
        }
    }
}

// MARK: Date

/// Swift's `Date`: an instant.
pub type Date = DateTime<Utc>;

/// 2001-01-01 00:00:00 UTC, which a Swift Date counts from, in Unix seconds.
const REFERENCE: f64 = 978_307_200.0;

/// `Date(timeIntervalSince1970:)`.
pub fn unix(seconds: f64) -> Date {
    let whole = seconds.floor();
    let nanos = (((seconds - whole) * 1e9).round() as u32).min(999_999_999);
    DateTime::from_timestamp(whole as i64, nanos).unwrap_or_else(distant_past)
}

/// `Date.timeIntervalSince1970`.
pub fn seconds(d: Date) -> f64 { d.timestamp() as f64 + d.timestamp_subsec_nanos() as f64 / 1e9 }

/// `Date.distantPast`: 0001-01-01.
pub fn distant_past() -> Date { DateTime::from_timestamp(-62_135_596_800, 0).expect("year 1") }

/// `addingTimeInterval(_:)`.
pub fn adding(d: Date, seconds: f64) -> Date { d + TimeDelta::nanoseconds((seconds * 1e9).round() as i64) }

/// A Date as `JSONEncoder` writes it by default: seconds since 2001-01-01 as a Double.
pub mod date {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> { s.serialize_f64(seconds(*d) - REFERENCE) }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> { Ok(unix(f64::deserialize(d)? + REFERENCE)) }
}

// MARK: UUID

/// Swift's `UUID`: written as `E621E1F8-C36C-495A-93FC-0C247A3E6E5F`, read in either case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    /// A random one (version 4), as `UUID()` makes.
    pub fn new() -> Self {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).expect("the system's randomness");
        b[6] = b[6] & 0x0f | 0x40;
        b[8] = b[8] & 0x3f | 0x80;
        Self(b)
    }

    /// `UUID(uuidString:)`.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() != 36 || [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') { return None }
        let hex: String = s.chars().filter(|&c| c != '-').collect();
        if hex.len() != 32 || !hex.bytes().all(|c| c.is_ascii_hexdigit()) { return None }
        let mut out = [0u8; 16];
        for (i, byte) in out.iter_mut().enumerate() { *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()? }
        Some(Self(out))
    }
}

impl Default for Uuid {
    fn default() -> Self { Self::new() }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let h: String = self.0.iter().map(|b| format!("{b:02X}")).collect();
        write!(f, "{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    }
}

impl Serialize for Uuid {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.collect_str(self) }
}

impl<'de> Deserialize<'de> for Uuid {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a UUID: {s}")))
    }
}

// MARK: Round 3: arithmetic that can't panic, `String(format:)`'s %g, compact dollars, ISO 8601

/// Swift's Decimal doesn't trap where rust_decimal panics: a result past rust_decimal's range, or divided by zero (NaN in
/// Swift), is None here and stays None through what follows. Money math on outside numbers (a chat, an API, a node)
/// goes through this: `Num::of(entry) * (1 - dir * s)`, then `.0?`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Num(pub Option<Decimal>);

impl Num {
    pub fn of(d: Decimal) -> Self { Self(Some(d)) }
    pub fn rounded(self, scale: u32, mode: Rounding) -> Self { Self(self.0.map(|d| rounded(d, scale, mode))) }
    pub fn abs(self) -> Self { Self(self.0.map(|d| d.abs())) }
}

impl From<Decimal> for Num {
    fn from(d: Decimal) -> Self { Self(Some(d)) }
}

macro_rules! num_op {
    ($op:ident, $f:ident, $checked:ident) => {
        impl std::ops::$op for Num {
            type Output = Num;
            fn $f(self, r: Num) -> Num { Num(self.0.zip(r.0).and_then(|(a, b)| a.$checked(b))) }
        }
        impl std::ops::$op<Decimal> for Num {
            type Output = Num;
            fn $f(self, r: Decimal) -> Num { std::ops::$op::$f(self, Num::of(r)) }
        }
        impl std::ops::$op<Num> for Decimal {
            type Output = Num;
            fn $f(self, r: Num) -> Num { std::ops::$op::$f(Num::of(self), r) }
        }
        impl std::ops::$op<i64> for Num {
            type Output = Num;
            fn $f(self, r: i64) -> Num { std::ops::$op::$f(self, Num::of(Decimal::from(r))) }
        }
        impl std::ops::$op<Num> for i64 {
            type Output = Num;
            fn $f(self, r: Num) -> Num { std::ops::$op::$f(Num::of(Decimal::from(self)), r) }
        }
    };
}
num_op!(Add, add, checked_add);
num_op!(Sub, sub, checked_sub);
num_op!(Mul, mul, checked_mul);
num_op!(Div, div, checked_div);

/// `NSDecimalNumber(decimal:).doubleValue`.
pub fn double(d: Decimal) -> f64 { text(d).parse().unwrap_or(0.0) }

/// `String(format: "%.<precision>g", x)`: `precision` significant digits, the shorter of fixed and exponent forms
/// (C's rule: the exponent form below 1e-4 or from 10^precision), trailing zeros dropped, «e-05» with two digits at
/// least.
pub fn format_g(x: f64, precision: usize) -> String {
    if x.is_nan() { return "nan".into() }
    if x.is_infinite() { return if x < 0.0 { "-inf".into() } else { "inf".into() } }
    let p = precision.max(1);
    let scientific = format!("{:.*e}", p - 1, x);
    let (mantissa, exp) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() };
    if exp < -4 || exp >= p as i32 {
        format!("{}e{}{:02}", trim(mantissa), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        trim(&format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, x))
    }
}

/// `.currency(code: "USD").notation(.compactName).precision(.significantDigits(3))` in en_US: «$19.6K», «$400K»,
/// «$0.500», «-$5.00», rounded half to even before the unit is picked (999,999 is «$1.00M»).
pub fn compact_usd(d: Decimal) -> String {
    let r = d.abs().round_sf_with_strategy(3, RoundingStrategy::MidpointNearestEven).unwrap_or(d.abs());
    let units = [(dec("1000000000000"), "T"), (dec("1000000000"), "B"), (dec("1000000"), "M"), (dec("1000"), "K")];
    let (v, unit) = units.iter().find(|(u, _)| r >= *u).map_or((r, ""), |(u, name)| (r / u, *name));
    // Places for three significant digits: the exponent of the leading digit.
    let exponent = if v.is_zero() { 0 } else {
        let t = text(v);
        let whole = t.split('.').next().unwrap_or("0");
        if whole != "0" { whole.len() as i32 - 1 } else { -(t.split('.').nth(1).unwrap_or("").chars().take_while(|&c| c == '0').count() as i32 + 1) }
    };
    let places = (2 - exponent).max(0) as u32;
    let fixed = rounded(v, places, Rounding::Bankers);
    let mut s = format!("{:.*}", places as usize, fixed);
    // Compact notation groups thousands only from five digits («1500T», «10,000T»).
    let whole_len = s.split('.').next().map_or(0, str::len);
    if whole_len >= 5 {
        let (whole, rest) = s.split_at(whole_len);
        let mut grouped = String::new();
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole_len - i) % 3 == 0 { grouped.push(',') }
            grouped.push(c);
        }
        s = grouped + rest;
    }
    format!("{}${s}{unit}", if d.is_sign_negative() && !r.is_zero() { "-" } else { "" })
}

/// `ISO8601DateFormatter().date(from:)`: «2026-09-06T04:36:17Z» or with «+03:00» / «+0300»; no fractions, no
/// lowercase «t» or «z».
pub fn iso8601(s: &str) -> Option<Date> {
    use chrono::NaiveDateTime;
    if let Some(utc) = s.strip_suffix('Z') {
        return NaiveDateTime::parse_from_str(utc, "%Y-%m-%dT%H:%M:%S").ok().map(|n| n.and_utc());
    }
    ["%Y-%m-%dT%H:%M:%S%:z", "%Y-%m-%dT%H:%M:%S%z"].iter().find_map(|f| DateTime::parse_from_str(s, f).ok()).map(|d| d.with_timezone(&Utc))
}

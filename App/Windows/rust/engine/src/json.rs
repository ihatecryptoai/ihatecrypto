//! Swift's JSONEncoder, the two ways the engine writes: its files with Foundation's defaults (a Date as seconds since
//! 2001) and the window's lines with `.iso8601` (`JSONEncoder.lines`, Outbox). One type serves both — a Message is in
//! canvases.json and in every state — so its dates follow the writer, as Swift's encoder strategy decides them.

use std::cell::Cell;

use ihatecrypto_core::foundation::{self, Date};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

thread_local! {
    /// Set while the window's JSON is written (`window`).
    static WINDOW: Cell<bool> = const { Cell::new(false) };
}

/// `value` as the window reads it: one JSON text, its dates in ISO 8601.
pub fn window<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Vec<u8>> {
    struct Back(bool);
    impl Drop for Back {
        fn drop(&mut self) { WINDOW.with(|w| w.set(self.0)) }
    }
    let _back = Back(WINDOW.with(|w| w.replace(true)));
    serde_json::to_vec(value)
}

/// 2001-01-01 00:00:00 UTC in Unix seconds: what a Swift Date counts from.
const REFERENCE: i64 = 978_307_200;

/// A Swift Date's Double, seconds since 2001, as an instant: whole seconds and nanoseconds apart, so it writes back as
/// the same Double (going through Unix seconds as one Double loses its last bit).
pub fn from_reference(t: f64) -> Option<Date> {
    if !t.is_finite() { return None }
    let whole = t.floor();
    let nanos = (((t - whole) * 1e9).round() as u32).min(999_999_999);
    chrono::DateTime::from_timestamp((whole as i64).checked_add(REFERENCE)?, nanos)
}

/// The instant as a Swift Date's Double.
pub fn to_reference(d: Date) -> f64 { (d.timestamp() - REFERENCE) as f64 + f64::from(d.timestamp_subsec_nanos()) / 1e9 }

/// `ISO8601DateFormatter`'s default: «2026-11-06T12:34:56Z», whole seconds, UTC.
pub fn iso(d: Date) -> String { d.format("%Y-%m-%dT%H:%M:%SZ").to_string() }

/// A Date (`#[serde(with = "crate::json::date")]`): seconds since 2001 in a file, ISO 8601 to the window. Read back from
/// either.
pub mod date {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        if WINDOW.with(Cell::get) { s.collect_str(&iso(*d)) } else { s.serialize_f64(to_reference(*d)) }
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Seconds(f64),
        Text(String),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        match Wire::deserialize(d)? {
            Wire::Seconds(t) => from_reference(t).ok_or_else(|| serde::de::Error::custom(format!("not a date: {t}"))),
            Wire::Text(s) => foundation::iso8601_fraction(&s).ok_or_else(|| serde::de::Error::custom(format!("not a date: {s}"))),
        }
    }

    /// An optional one, left out when nil (`#[serde(default, skip_serializing_if = "Option::is_none", with = …)]`).
    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
            match d { Some(d) => super::serialize(d, s), None => s.serialize_none() }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
            #[derive(Deserialize)]
            struct Wrap(#[serde(with = "super")] Date);
            Ok(Option::<Wrap>::deserialize(d)?.map(|w| w.0))
        }
    }
}

/// A `[String: Decimal]` (`#[serde(with = "crate::json::numbers")]`): each value a number with every digit, as
/// `foundation::number` writes one.
pub mod numbers {
    use std::collections::HashMap;

    use ihatecrypto_core::foundation::number;
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    struct Out<'a>(&'a Decimal);

    impl Serialize for Out<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { number::serialize(self.0, s) }
    }

    #[derive(Deserialize)]
    struct In(#[serde(with = "number")] Decimal);

    pub fn serialize<S: Serializer>(map: &HashMap<String, Decimal>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(map.iter().map(|(k, v)| (k, Out(v))))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<HashMap<String, Decimal>, D::Error> {
        Ok(HashMap::<String, In>::deserialize(d)?.into_iter().map(|(k, v)| (k, v.0)).collect())
    }
}

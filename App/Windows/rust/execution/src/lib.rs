//! The envelope a person approves and the write-before-send journal (`App/Sources/Execution`: Envelope.swift,
//! Journal.swift). The executor that runs an approved plan (`Execution/Trade`) comes after the core it needs.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What the person approves: limits, not a price snapshot. Live values may move inside the envelope without asking
/// again; anything outside stops the route and needs a new approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    /// Most that may leave the source wallet, in source units.
    pub max_input: Decimal,
    /// Least that must arrive, in destination units.
    pub min_output: Decimal,
    /// Exact address funds may go to, once known (a quote's deposit address, the person's own account).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(with = "swift_date")]
    pub expires: DateTime<Utc>,
}

impl Envelope {
    /// From a quote the person saw: the same input, the output less `tolerance`, valid for `ttl_seconds`.
    pub fn approve(input: Decimal, quoted_output: Decimal, tolerance: Decimal, ttl_seconds: i64, now: DateTime<Utc>) -> Self {
        Envelope { max_input: input, min_output: quoted_output * (Decimal::ONE - tolerance), destination: None, expires: now + Duration::seconds(ttl_seconds) }
    }

    /// The Swift defaults: 1 % tolerance, ten minutes.
    pub fn approve_default(input: Decimal, quoted_output: Decimal, now: DateTime<Utc>) -> Self {
        Self::approve(input, quoted_output, Decimal::new(1, 2), 600, now)
    }

    /// Empty when the live values fit. Each string says what is out of bounds, for the person.
    pub fn violations(&self, input: Decimal, output: Decimal, destination: Option<&str>, now: DateTime<Utc>) -> Vec<String> {
        let mut out = Vec::new();
        if now > self.expires { out.push("The confirmation expired.".into()) }
        if input > self.max_input { out.push(format!("Needs to send {input}, but at most {} was approved.", self.max_input)) }
        if output < self.min_output { out.push(format!("{output} would arrive, but at least {} was approved.", self.min_output)) }
        if let (Some(approved), Some(destination)) = (&self.destination, destination) {
            if approved != destination { out.push(format!("Recipient address {destination} doesn't match the approved {approved}.")) }
        }
        out
    }
}

/// A date as Swift's `Codable` writes it by default: seconds since 2001-01-01, a JSON number.
pub mod swift_date {
    use super::*;

    pub const REFERENCE: i64 = 978_307_200;

    pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64((d.timestamp_micros() - REFERENCE * 1_000_000) as f64 / 1e6)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let secs = f64::deserialize(d)?;
        Utc.timestamp_micros(((secs + REFERENCE as f64) * 1e6).round() as i64).single().ok_or_else(|| serde::de::Error::custom("not a date"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind { Signed, Confirmed, Failed, Expired, Settled }

/// One event of the journal. `at` crosses the file as ISO 8601 to the second, as Swift's `.iso8601` strategy writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(with = "iso8601")]
    pub at: DateTime<Utc>,
    pub chain: String,
    pub id: String,
    pub kind: Kind,
    pub note: String,
}

impl Entry {
    pub fn new(chain: &str, id: &str, kind: Kind, note: &str) -> Self {
        Entry { at: Utc::now(), chain: chain.into(), id: id.into(), kind, note: note.into() }
    }
}

mod iso8601 {
    use super::*;

    pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let s = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)).map_err(serde::de::Error::custom)
    }
}

/// Append-only log of everything signed. An entry is written and flushed to disk before the transaction is broadcast,
/// keyed by an id known in advance (Solana signature, Hyperliquid nonce, 1Click deposit address). After a crash,
/// `unresolved()` lists what must be checked on chain instead of being resent.
/// ponytail: one JSON line per event in a file; move to SQLite when the app needs queries over history.
#[derive(Debug, Clone)]
pub struct Journal {
    pub path: PathBuf,
}

/// What the journal says when its file can't be read or written.
pub type JournalResult<T> = std::result::Result<T, String>;

impl Journal {
    pub fn new(path: impl Into<PathBuf>) -> Self { Journal { path: path.into() } }

    /// Written and synced to disk before it returns: what the caller then broadcasts is on record first.
    pub fn append(&self, entry: &Entry) -> JournalResult<()> {
        let mut line = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
        line.push(b'\n');
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&self.path).map_err(|e| format!("The journal can't be opened: {e}"))?;
        file.write_all(&line).and_then(|_| file.sync_all()).map_err(|e| format!("The journal can't be written: {e}"))
    }

    pub fn entries(&self) -> JournalResult<Vec<Entry>> {
        let Ok(data) = std::fs::read(&self.path) else { return Ok(vec![]) };
        data.split(|&b| b == b'\n').filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).map_err(|e| format!("A journal line can't be read: {e}"))).collect()
    }

    /// Each chain|id once, in the order first seen, with its latest entry and its signed one.
    fn folded(&self) -> JournalResult<Vec<(Entry, Option<Entry>)>> {
        let (mut latest, mut signed, mut order): (HashMap<String, Entry>, HashMap<String, Entry>, Vec<String>) = Default::default();
        for entry in self.entries()? {
            let key = format!("{}|{}", entry.chain, entry.id);
            if !latest.contains_key(&key) { order.push(key.clone()) }
            if entry.kind == Kind::Signed { signed.insert(key.clone(), entry.clone()); }
            latest.insert(key, entry);
        }
        Ok(order.into_iter().map(|k| (latest.remove(&k).expect("seen"), signed.remove(&k))).collect())
    }

    /// Signed, but no later confirmed/failed/expired/settled entry for the same chain and id. In file order.
    pub fn unresolved(&self) -> JournalResult<Vec<Entry>> {
        Ok(self.folded()?.into_iter().map(|(e, _)| e).filter(|e| e.kind == Kind::Signed).collect())
    }

    /// What went through: confirmed, or settled where it landed — each with the note and the time it was signed with, and
    /// the id an explorer knows it by (a swap with no gas of ours landed under the sponsor's signature). In file order.
    /// Steps that move no money aren't counted, so a refusal after one of them is still one the person may try again: an
    /// approve, a Permit2 allowance, a cleared allowance or a revoke on its own (an EIP-7702 batch «Approve … + …» moves
    /// its call's money), and on Hyperliquid anything but a fill or a transfer.
    pub fn went_through(&self) -> JournalResult<Vec<Entry>> {
        Ok(self.folded()?.into_iter().filter_map(|(e, signed)| {
            if !matches!(e.kind, Kind::Confirmed | Kind::Settled) { return None }
            let s = signed.unwrap_or_else(|| e.clone());
            let note = s.note.to_lowercase();
            if ["approve", "permit2:", "clear ", "revoke"].iter().any(|p| note.starts_with(p)) && !note.contains(" + ") { return None }
            if e.chain == "hyperliquid" && !e.note.starts_with("filled")
                && !["sendasset", "usdclasstransfer", "send ", "hlp "].iter().any(|p| note.starts_with(p)) { return None }
            let landed = match e.note.strip_prefix("transaction ") { Some(t) if e.chain == "solana" => t.to_string(), _ => e.id.clone() };
            Some(Entry { at: s.at, chain: e.chain, id: landed, kind: e.kind, note: s.note })
        }).collect())
    }

    /// Money on its way between networks: a bridge order (Relay's, 1Click's, a CCTP burn, a USDT0 send on LayerZero)
    /// signed within `within_seconds` and not yet settled, refunded or failed. The newest first.
    pub fn in_transit(&self, within_seconds: i64, now: DateTime<Utc>) -> JournalResult<Vec<Entry>> {
        let mut out: Vec<Entry> = self.unresolved()?.into_iter()
            .filter(|e| ["relay", "1click", "cctp", "layerzero"].contains(&e.chain.as_str()) && (now - e.at).num_milliseconds() < within_seconds * 1000)
            .collect();
        out.reverse();
        Ok(out)
    }
}

#[cfg(test)]
mod tests;

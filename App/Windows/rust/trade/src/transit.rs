//! Money on its way between networks (Transit.swift): the bridge orders the journal still has open, asked of their bridge
//! once they're old enough; and the landings of the bridge legs a chain of moves signed.

use std::future::Future;
use std::sync::Mutex;

use chrono::{DateTime, TimeDelta, Utc};
use execution::{Entry, Kind};
use protocols::{one_click, relay};
use tokio::task::JoinHandle;

use crate::{Error, Executor, Result};

impl Executor {
    /// The bridge orders still on their way (Journal.in_transit), each older than two minutes asked of its bridge first:
    /// one that settled, refunded or failed meanwhile — the app closed before it heard — gets its end written to the
    /// journal and drops off. What's left is really in transit, the newest first.
    pub async fn resolve_transit(&self, now: DateTime<Utc>) -> Vec<Entry> {
        let Ok(open) = self.journal.in_transit(86_400, now) else { return vec![] };
        let mut left = vec![];
        for entry in open {
            let age = now - entry.at;
            if age <= TimeDelta::seconds(120) {
                left.push(entry);
                continue;
            }
            let mut end: Option<(Kind, String)> = None;
            match entry.chain.as_str() {
                "relay" => {
                    if let Ok((s, _)) = relay::status(&entry.id).await {
                        if s == "success" { end = Some((Kind::Settled, s)) } else if ["refund", "failure"].contains(&s.as_str()) { end = Some((Kind::Failed, s)) }
                    }
                }
                "cctp" => {
                    if let Some((source, burn)) = entry.id.split_once(':') {
                        if let Ok(source) = source.parse::<u32>() {
                            if let Some(mint) = Self::cctp_forward(source, burn).await { end = Some((Kind::Settled, mint)) }
                        }
                    }
                }
                "layerzero" => {
                    if let Some((status, delivered)) = Self::layer_zero_status(&entry.id).await {
                        if status == "DELIVERED" { end = Some((Kind::Settled, delivered.unwrap_or(status))) }
                        else if status == "FAILED" { end = Some((Kind::Failed, status)) }
                    }
                }
                "1click" => {
                    if let Ok(s) = one_click::status(&entry.id).await {
                        if s.is_final() {
                            end = Some((if s.status == "SUCCESS" { Kind::Settled } else { Kind::Failed }, s.status));
                        } else if s.status == "PENDING_DEPOSIT" && entry.note.starts_with("Receive:") && age > TimeDelta::seconds(65 * 60) {
                            // A receiving address nobody sent to by its hour's deadline is over.
                            end = Some((Kind::Expired, "nothing came by the deadline".into()));
                        }
                    }
                }
                _ => {}
            }
            match end {
                Some((kind, status)) => { let _ = self.record(&entry.chain, &entry.id, kind, &format!("{status} (seen later)")); }
                None => left.push(entry),
            }
        }
        left
    }
}

/// Landings of the bridge legs a chain signed (`crate::LANDINGS`): each leg's money lands while the next leg is signed,
/// so a chain bringing USDC from three places waits for the slowest bridge, not the three in a row. A step that spends
/// where they land waits for those first; a landing that failed throws there.
#[derive(Default)]
pub struct Landings {
    pending: Mutex<Vec<(String, JoinHandle<Result<String>>)>>,
}

impl Landings {
    pub fn new() -> Self { Self::default() }

    /// `wait` lands the leg and says how it went («Moved 2 USDC … arrived»); it starts at once.
    pub(crate) fn add(&self, place: &str, wait: impl Future<Output = Result<String>> + Send + 'static) {
        let task = tokio::spawn(wait);
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).push((place.into(), task));
    }

    /// How many are still out at `place` (None: anywhere).
    pub fn count(&self, place: Option<&str>) -> usize {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(p, _)| place.is_none_or(|w| w == p)).count()
    }

    /// Waits for the landings at `place` (None: all of them) and gives what each said; the first that failed throws.
    pub async fn wait(&self, place: Option<&str>) -> Result<Vec<String>> {
        let due: Vec<JoinHandle<Result<String>>> = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let (due, kept) = std::mem::take(&mut *pending).into_iter().partition(|(p, _)| place.is_none_or(|w| w == p));
            *pending = kept;
            due.into_iter().map(|(_, task)| task).collect()
        };
        let mut said = vec![];
        for task in due { said.push(task.await.map_err(|e| Error::Execution(e.to_string()))??) }
        Ok(said)
    }
}

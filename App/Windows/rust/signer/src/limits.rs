//! The person's limits (Settings), kept in the signer's trusted settings (`Limits.swift`): the engine can lower them,
//! and raises them only through a Windows Hello prompt that names the new numbers (`SetLimits`). And what the trading
//! key and the automation wallet spent without the person, in a log of their own (`budget.json`).
//!
//! On a Mac those entries live in the signer's own Keychain item, which nothing but the signer's code signature can
//! touch. Here any program of the same Windows user can write the signer's folder, so the log is sealed with an HMAC under
//! a key from Windows Hello: the budget subkey of the key any Windows Hello in this run gave (an app unlock, a signature,
//! a confirmation), held until the signer exits. It seals only this log — never a seed or the trusted settings. A run
//! with no Windows Hello yet asks once when a budget must be written (the automation wallet's 60-second prompt). The log
//! fails closed: one that doesn't verify, is gone while the trusted settings say there is one, or changed under this run
//! (copied back, or written by another signer) counts as the whole day and week spent, so the move asks the person.
//! ponytail: a sealed log copied back from earlier between two runs isn't told apart, as for the whole trusted state
//! (trusted.rs); a TPM monotonic counter through winsec would.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use signer_api::SignerLimits;
use wallet::{Result, WalletError};

use crate::trusted::{Pref, prefs};
use crate::vault::{self, Key};

pub fn current() -> SignerLimits { from(prefs::data("limits").as_deref()) }

/// The limits a stored value holds: unreadable or out of range reads as the standard ones.
pub fn from(data: Option<&[u8]>) -> SignerLimits {
    data.and_then(|d| serde_json::from_slice::<SignerLimits>(d).ok()).filter(|l| l.problem().is_none()).unwrap_or(SignerLimits::STANDARD)
}

pub fn save(limits: &SignerLimits) { prefs::set("limits", serde_json::to_vec(limits).ok().map(Pref::Data)) }

/// What is kept: 7 days of entries.
pub const KEPT: f64 = 7.0 * 86_400.0;
/// The log's file in the signer's folder.
pub const LOG: &str = "budget.json";
/// Set in the sealed trusted settings once there is a log: from then on a missing one is a deleted one.
pub const ANCHOR: &str = "budgetLog";
/// What a log that can't be trusted counts as, in dollars: more than any limit, for a day and a week.
const SPENT: f64 = 1e12;

/// Written down, or not: then the move it counts isn't signed (the Mac's message when SignerPrefs.set fails).
pub const UNWRITTEN: &str = "The signer couldn't write down what this move spends: nothing was signed.";

fn now() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()) }

/// Dollars signed without the person, newest last, each [Unix seconds, dollars]: what the day and the week add up.
/// ponytail: one list per budget for the whole signer (every wallet's trading key shares «agent»), stricter than one
/// per wallet; split it per wallet if someone runs several trading keys at full size.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Log {
    agent: Vec<[f64; 2]>,
    automation: Vec<[f64; 2]>,
}

impl Log {
    /// The whole day and week spent, from now.
    fn spent() -> Log { let at = now(); Log { agent: vec![[at, SPENT]], automation: vec![[at, SPENT]] } }

    fn list(&mut self, budget: Budget) -> &mut Vec<[f64; 2]> { if budget == Budget::AGENT { &mut self.agent } else { &mut self.automation } }

    fn spent_in(&self, budget: Budget, seconds: f64) -> Decimal {
        let at = now();
        let list = if budget == Budget::AGENT { &self.agent } else { &self.automation };
        list.iter().filter(|e| at - e[0] < seconds && e[1] > 0.0)
            .fold(Decimal::ZERO, |sum, e| sum.saturating_add(Decimal::from_f64(e[1]).unwrap_or(Decimal::MAX)))
    }
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    #[serde(with = "wallet::b64")]
    body: Vec<u8>,
    #[serde(with = "wallet::b64")]
    mac: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget(&'static str);

impl Budget {
    pub const AGENT: Budget = Budget("agent");
    pub const AUTOMATION: Budget = Budget("automation");
}

/// Checking a budget and taking from it is one step under this lock, so moves sent together can't each pass before any
/// is counted (the 2026-11-08 review); across signer processes the log's own file lock does the same.
static TAKING: Mutex<()> = Mutex::new(());
/// This run's budget key, from the first Windows Hello it got.
static RUN: Mutex<Option<Key>> = Mutex::new(None);
/// The log's body as this run last read or wrote it: one that differs on disk was changed under it.
static LAST: Mutex<Option<Vec<u8>>> = Mutex::new(None);

fn taking() -> std::sync::MutexGuard<'static, ()> { TAKING.lock().unwrap_or_else(|e| e.into_inner()) }

/// The log's file lock, held while it's read and written: several signer processes can't each keep their own count.
fn file_lock() -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(vault::path("budget.lock"))
        .map_err(|_| WalletError(UNWRITTEN.into()))?;
    file.lock().map_err(|_| WalletError(UNWRITTEN.into()))?;
    Ok(file)
}

fn mac(key: &Key, body: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&**key).expect("any key");
    m.update(body);
    m.finalize().into_bytes().to_vec()
}

/// The log as this run may trust it: verified under its key, whole, and what this run last had — else the whole day and
/// week spent, written so (a log that fails stays failed after a restart).
fn read(key: &Key) -> Log {
    let file = vault::read(LOG).and_then(|d| serde_json::from_slice::<Sealed>(&d).ok());
    let verified = file.filter(|s| {
        let ok = <Hmac<Sha256> as Mac>::new_from_slice(&**key).expect("any key").chain_update(&s.body).verify_slice(&s.mac).is_ok();
        ok && LAST.lock().expect("last").as_ref().is_none_or(|last| *last == s.body)
    });
    match verified.and_then(|s| serde_json::from_slice::<Log>(&s.body).ok().map(|log| (log, s.body))) {
        Some((mut log, body)) => {
            *LAST.lock().expect("last") = Some(body);
            let at = now();
            for list in [&mut log.agent, &mut log.automation] { list.retain(|e| at - e[0] < KEPT) }
            log
        }
        None => {
            let log = Log::spent();
            let _ = write(key, &log);
            log
        }
    }
}

fn write(key: &Key, log: &Log) -> Result<()> {
    let body = serde_json::to_vec(log).map_err(|_| WalletError(UNWRITTEN.into()))?;
    let sealed = serde_json::to_vec(&Sealed { mac: mac(key, &body), body: body.clone() }).map_err(|_| WalletError(UNWRITTEN.into()))?;
    vault::write(LOG, &sealed).map_err(|_| WalletError(UNWRITTEN.into()))?;
    *LAST.lock().expect("last") = Some(body);
    Ok(())
}

/// A Windows Hello in this run gave `master` (vault::key, vault::confirm): its budget subkey is kept for the run, and the
/// log is made if there's none yet — a fresh one the first time, the whole day and week spent if the trusted settings say
/// there was one. Call with `master` in the request's scope: the anchor is sealed with it.
pub fn opened(master: &Key) {
    let key = vault::subkey(master, "budget");
    *RUN.lock().expect("run") = Some(key.clone());
    let _taking = taking();
    let Ok(_file) = file_lock() else { return };
    let anchored = prefs::bool(ANCHOR);
    if !vault::exists(LOG) && write(&key, &if anchored { Log::spent() } else { Log::default() }).is_err() { return }
    if !anchored { prefs::set(ANCHOR, Some(Pref::Bool(true))) }
}

/// For tests: forget the run's key and what it read, as a new signer process would.
pub fn forget() {
    *RUN.lock().expect("run") = None;
    *LAST.lock().expect("last") = None;
}

/// Why a budget can't be checked in a run that hasn't had Windows Hello yet: the person then allows the move.
fn unopened(dollars: Decimal) -> String {
    format!("what moves spent opens with Windows Hello once after ihatecrypto starts (this move ≈ ${})", usd(dollars))
}

/// Dollars a move took from a budget, written down already: kept when the move is signed (`keep`), given back when it's
/// dropped unsigned.
#[must_use]
pub struct Hold {
    budget: Budget,
    entry: Option<[f64; 2]>,
}

impl Hold {
    /// The move was signed: its dollars stay counted.
    pub fn keep(mut self) { self.entry = None }
}

impl Drop for Hold {
    /// Given back: the entry out of the log, if it still verifies. If it doesn't, it stays counted (fails closed).
    fn drop(&mut self) {
        let Some(entry) = self.entry.take() else { return };
        let Some(key) = RUN.lock().expect("run").clone() else { return };
        let _taking = taking();
        let Ok(_file) = file_lock() else { return };
        let mut log = read(&key);
        let list = log.list(self.budget);
        if let Some(i) = list.iter().rposition(|e| *e == entry) {
            list.remove(i);
            let _ = write(&key, &log);
        }
    }
}

/// Takes `dollars` from `budget`, checked against `over` when given: Ok(Ok) taken and written down, Ok(Err(why)) not
/// within the limits (nothing taken; the person may allow it), Err the log couldn't be written (the move isn't signed).
fn take_with(budget: Budget, dollars: Decimal, over: Option<&dyn Fn(&Log) -> Option<String>>) -> Result<std::result::Result<Hold, String>> {
    let _taking = taking();
    let Some(key) = RUN.lock().expect("run").clone() else {
        return if over.is_some() { Ok(Err(unopened(dollars))) } else { Err(WalletError(UNWRITTEN.into())) };
    };
    let _file = file_lock()?;
    let mut log = read(&key);
    if let Some(why) = over.and_then(|over| over(&log)) { return Ok(Err(why)) }
    if dollars <= Decimal::ZERO { return Ok(Ok(Hold { budget, entry: None })) }
    let entry = [now(), dollars.to_f64().unwrap_or(f64::MAX)];
    log.list(budget).push(entry);
    write(&key, &log)?;
    Ok(Ok(Hold { budget, entry: Some(entry) }))
}

/// Takes `dollars` from the trading key's day if they fit, else why not (nothing taken).
pub fn take_agent(dollars: Decimal, limits: &SignerLimits) -> Result<std::result::Result<Hold, String>> {
    take_with(Budget::AGENT, dollars, Some(&|log: &Log| agent_over(log, dollars, limits)))
}

/// Takes `dollars` from the automation wallet's day and week if they fit, else why not (nothing taken).
pub fn take_automation(dollars: Decimal, limits: &SignerLimits) -> Result<std::result::Result<Hold, String>> {
    take_with(Budget::AUTOMATION, dollars, Some(&|log: &Log| automation_over(log, dollars, limits)))
}

/// Counts `dollars` the person allowed in person, past the limits (their prompt opened this run's key).
pub fn take(budget: Budget, dollars: Decimal) -> Result<Hold> {
    take_with(budget, dollars, None)?.map_err(WalletError)
}

/// The log was changed outside the signer (or lost): what it counts as, in words.
fn untrusted(dollars: Decimal) -> String {
    format!("the signer's record of what moves spent was changed outside it, so this day and week count as spent (this move ≈ ${})", usd(dollars))
}

/// Why the trading key can't open `dollars` more on its own, or None: over one order, or over the day with what it
/// already opened.
fn agent_over(log: &Log, dollars: Decimal, limits: &SignerLimits) -> Option<String> {
    if log.spent_in(Budget::AGENT, KEPT) >= Decimal::from_f64(SPENT).unwrap_or(Decimal::MAX) { return Some(untrusted(dollars)) }
    if dollars > Decimal::from(limits.agent_order) {
        return Some(format!("over the trading key's ${} an order (this one ≈ ${})", limits.agent_order, usd(dollars)));
    }
    let today = log.spent_in(Budget::AGENT, 86_400.0);
    if today.saturating_add(dollars) > Decimal::from(limits.agent_day) {
        return Some(format!("over the trading key's ${} a day (${} opened today, this one ≈ ${})", limits.agent_day, usd(today), usd(dollars)));
    }
    None
}

/// Why the automation wallet can't spend `dollars` more without the person, or None.
fn automation_over(log: &Log, dollars: Decimal, limits: &SignerLimits) -> Option<String> {
    let (day, week) = (log.spent_in(Budget::AUTOMATION, 86_400.0), log.spent_in(Budget::AUTOMATION, KEPT));
    if week >= Decimal::from_f64(SPENT).unwrap_or(Decimal::MAX) { return Some(untrusted(dollars)) }
    if day.saturating_add(dollars) > Decimal::from(limits.automation_day) {
        return Some(format!("over the automation wallet's ${} a day (${} spent today, this move ≈ ${})", limits.automation_day, usd(day), usd(dollars)));
    }
    if week.saturating_add(dollars) > Decimal::from(limits.automation_week) {
        return Some(format!("over the automation wallet's ${} a week (${} spent this week, this move ≈ ${})", limits.automation_week, usd(week), usd(dollars)));
    }
    None
}

/// Cents, rounded up: 12.341 → «12.35», 150 → «150».
pub fn usd(d: Decimal) -> String { d.round_dp_with_strategy(2, RoundingStrategy::AwayFromZero).normalize().to_string() }

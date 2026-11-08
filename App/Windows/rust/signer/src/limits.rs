//! The person's limits (Settings), kept in the signer's trusted settings (`Limits.swift`): the engine can lower them,
//! and raises them only through a Windows Hello prompt that names the new numbers (`SetLimits`).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::{Decimal, RoundingStrategy};
use signer_api::SignerLimits;

use wallet::WalletError;

use crate::trusted::{self, Pref, prefs};

pub fn current() -> SignerLimits { from(prefs::data("limits").as_deref()) }

/// The limits a stored value holds: unreadable or out of range reads as the standard ones.
pub fn from(data: Option<&[u8]>) -> SignerLimits {
    data.and_then(|d| serde_json::from_slice::<SignerLimits>(d).ok()).filter(|l| l.problem().is_none()).unwrap_or(SignerLimits::STANDARD)
}

pub fn save(limits: &SignerLimits) { prefs::set("limits", serde_json::to_vec(limits).ok().map(Pref::Data)) }

/// What is kept: 7 days of entries.
pub const KEPT: f64 = 7.0 * 86_400.0;

fn now() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()) }

/// The entries a stored budget holds, [Unix seconds, dollars] each.
pub fn entries_in(data: Option<&[u8]>) -> Vec<[f64; 2]> { data.and_then(|d| serde_json::from_slice(d).ok()).unwrap_or_default() }

/// Dollars signed without the person, newest last, each with its time: what the day and the week add up. Kept for 7
/// days. ponytail: one list per budget for the whole signer (every wallet's trading key shares «agent»), stricter than
/// one per wallet; split it per wallet if someone runs several trading keys at full size.
#[derive(Clone, Copy)]
pub struct Budget(&'static str);

impl Budget {
    pub const AGENT: Budget = Budget("spent-agent");
    pub const AUTOMATION: Budget = Budget("spent-automation");

    fn entries(&self, now: f64) -> Vec<[f64; 2]> {
        entries_in(prefs::data(self.0).as_deref()).into_iter().filter(|e| now - e[0] < KEPT).collect()
    }

    /// Spent in the last `seconds`, with what moves under way hold.
    pub fn spent(&self, seconds: f64) -> Decimal {
        let now = now();
        let written = self.entries(now).iter().filter(|e| now - e[0] < seconds && e[1] > 0.0)
            .fold(Decimal::ZERO, |sum, e| sum.saturating_add(Decimal::from_f64(e[1]).unwrap_or(Decimal::MAX)));
        written.saturating_add(HELD.lock().expect("held").get(self.0).copied().unwrap_or_default())
    }

    /// Written down, or why it couldn't be: then the move it counts isn't handed out.
    fn record(&self, dollars: Decimal) -> wallet::Result<()> {
        if dollars <= Decimal::ZERO { return Ok(()) }
        let now = now();
        let entry = [now, dollars.to_f64().unwrap_or(f64::MAX)];
        let mut all = self.entries(now);
        all.push(entry);
        let data = serde_json::to_vec(&all).map_err(|e| WalletError(e.to_string()))?;
        // Only ever more spent, so it never waits for Windows Hello (trusted::takes_trust_away); read back to be sure.
        trusted::mutate(None, |s| { s.prefs.insert(self.0.into(), Pref::Data(data)); })?;
        if !entries_in(prefs::data(self.0).as_deref()).contains(&entry) {
            return Err(WalletError("The signer couldn't count this move against your limits, so it wasn't signed.".into()));
        }
        Ok(())
    }
}

/// ponytail: what's spent with no Windows Hello key in hand waits in pending.json, which a program of this Windows user
/// can delete between two signer runs, as it can a pause or a rename kept there (trusted.rs): the budget then starts
/// over. Holding it would take a key no one is asked for (seal the entries with a key kept for the run, and count the
/// day as spent until it's opened) — the owner's call.
/// Checking a budget and taking from it is one step under this lock, so moves sent together can't each pass before any
/// is counted (the 2026-11-08 review). The signer serves one request at a time today; this holds if that changes.
static TAKING: Mutex<()> = Mutex::new(());
/// What moves under way hold, by budget: in this process only, so the written list only ever grows (a refund written
/// back would give trust back, which pending.json can't carry).
static HELD: Mutex<BTreeMap<&'static str, Decimal>> = Mutex::new(BTreeMap::new());

/// Dollars a move took from a budget: written down when it's signed (`keep`), given back when dropped unsigned.
#[must_use]
pub struct Hold {
    budget: Budget,
    dollars: Decimal,
}

impl Hold {
    fn new(budget: Budget, dollars: Decimal) -> Hold {
        if dollars > Decimal::ZERO { *HELD.lock().expect("held").entry(budget.0).or_default() += dollars }
        Hold { budget, dollars: dollars.max(Decimal::ZERO) }
    }

    /// The move was signed: its dollars count from now on. An error: they couldn't be written down, and the caller
    /// doesn't hand out the signature.
    pub fn keep(mut self) -> wallet::Result<()> {
        let _taking = TAKING.lock().unwrap_or_else(|e| e.into_inner());
        let written = self.budget.record(self.dollars);
        self.release();
        written
    }

    fn release(&mut self) {
        if let Some(held) = HELD.lock().expect("held").get_mut(self.budget.0) { *held -= self.dollars }
        self.dollars = Decimal::ZERO;
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let _taking = TAKING.lock().unwrap_or_else(|e| e.into_inner());
        self.release();
    }
}

/// Takes `dollars` from the trading key's day if they fit, else why not (nothing taken).
pub fn take_agent(dollars: Decimal, limits: &SignerLimits) -> Result<Hold, String> {
    let _taking = TAKING.lock().unwrap_or_else(|e| e.into_inner());
    match agent_over(dollars, limits) { Some(why) => Err(why), None => Ok(Hold::new(Budget::AGENT, dollars)) }
}

/// Takes `dollars` from the automation wallet's day and week if they fit, else why not (nothing taken).
pub fn take_automation(dollars: Decimal, limits: &SignerLimits) -> Result<Hold, String> {
    let _taking = TAKING.lock().unwrap_or_else(|e| e.into_inner());
    match automation_over(dollars, limits) { Some(why) => Err(why), None => Ok(Hold::new(Budget::AUTOMATION, dollars)) }
}

/// Counts `dollars` the person allowed in person, past the limits.
pub fn take(budget: Budget, dollars: Decimal) -> Hold {
    let _taking = TAKING.lock().unwrap_or_else(|e| e.into_inner());
    Hold::new(budget, dollars)
}

/// Why the trading key can't open `dollars` more on its own, or None: over one order, or over the day with what it
/// already opened.
fn agent_over(dollars: Decimal, limits: &SignerLimits) -> Option<String> {
    if dollars > Decimal::from(limits.agent_order) {
        return Some(format!("over the trading key's ${} an order (this one ≈ ${})", limits.agent_order, usd(dollars)));
    }
    let today = Budget::AGENT.spent(86_400.0);
    if today.saturating_add(dollars) > Decimal::from(limits.agent_day) {
        return Some(format!("over the trading key's ${} a day (${} opened today, this one ≈ ${})", limits.agent_day, usd(today), usd(dollars)));
    }
    None
}

/// Why the automation wallet can't spend `dollars` more without the person, or None.
fn automation_over(dollars: Decimal, limits: &SignerLimits) -> Option<String> {
    let (day, week) = (Budget::AUTOMATION.spent(86_400.0), Budget::AUTOMATION.spent(KEPT));
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

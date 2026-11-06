//! «Переложи в лучшие ставки» (`Rebalance.swift`): money already deposited moves toward the best rates, no protocol
//! above `max_share` of the total, and a move is made only when its extra interest pays back the way out and in
//! (bridges, first-deposit rent, gas) within `payback_days`. A locked place (HLP's 4 days) keeps its money. Pure, so
//! the rules are tested apart from the chain.

use std::collections::HashMap;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::two_places;

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    /// % a year.
    pub apy: Decimal,
    /// USDC in it now.
    pub amount: Decimal,
    /// Dollars it costs to take money out to Solana (a bridge back), and why it can't leave now (a lock), or nil.
    pub exit_cost: Decimal,
    pub locked: Option<String>,
    /// Dollars it costs to put money in (a bridge there, first-deposit rent, gas), and why ihatecrypto won't, or nil.
    pub entry_cost: Decimal,
    pub closed: Option<String>,
    /// The protocol the limit is counted per («Aave» on every network): see allocation::Place.
    pub protocol_name: String,
}

impl Place {
    /// Free to leave and enter, its own protocol: `Place { locked, entry_cost, …, ..Place::new(…) }` for the rest.
    pub fn new(name: &str, apy: Decimal, amount: Decimal) -> Self {
        Self { name: name.into(), apy, amount, exit_cost: Decimal::ZERO, locked: None, entry_cost: Decimal::ZERO, closed: None, protocol_name: name.into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub from: String,
    pub to: String,
    pub amount: Decimal,
    pub from_apy: Decimal,
    pub to_apy: Decimal,
    /// Dollars: leaving `from` plus entering `to`.
    pub cost: Decimal,
}

impl Move {
    /// Extra interest a year on the moved amount.
    pub fn yearly_gain(&self) -> Decimal { self.amount * (self.to_apy - self.from_apy) / Decimal::ONE_HUNDRED }

    /// Days until the extra interest has paid the cost; nil when it never does.
    pub fn payback_days(&self) -> Option<Decimal> {
        let gain = self.yearly_gain();
        (gain > Decimal::ZERO).then(|| self.cost / gain * Decimal::from(365))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skip {
    /// Swift's `move`, a keyword here.
    pub step: Move,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub moves: Vec<Move>,
    /// Considered and not made, each with the reason (the math when it's the cost).
    pub skipped: Vec<Skip>,
    pub notes: Vec<String>,
}

impl Plan {
    pub fn yearly_gain(&self) -> Decimal { self.moves.iter().map(Move::yearly_gain).sum() }
    pub fn cost(&self) -> Decimal { self.moves.iter().map(|m| m.cost).sum() }
}

/// With Swift's defaults: a move pays back within 30 days, and none under 0.5 USDC.
pub fn plan(places: &[Place], max_share: Decimal) -> Plan { plan_with(places, max_share, Decimal::from(30), Decimal::new(5, 1)) }

/// `min_move`: smaller pieces are not worth a transaction.
pub fn plan_with(places: &[Place], max_share: Decimal, payback_days: Decimal, min_move: Decimal) -> Plan {
    let total: Decimal = places.iter().map(|p| p.amount).sum();
    if total <= Decimal::ZERO {
        return Plan { moves: vec![], skipped: vec![], notes: vec!["Nothing is deposited yet: «spread USDC across the best rates» puts free USDC to work.".into()] };
    }
    let cap = total * max_share / Decimal::ONE_HUNDRED;
    let mut notes = Vec::new();
    let half = min_move / Decimal::TWO;
    // Targets: the best rates fill up to the cap first; a locked place keeps what it has, a closed one can only shrink.
    let mut remaining = total - places.iter().filter(|p| p.locked.is_some()).map(|p| p.amount).sum::<Decimal>();
    let mut target: HashMap<&str, Decimal> = HashMap::new();
    // The cap is per protocol: Aave on two networks shares one. Locked money counts first.
    let mut used: HashMap<&str, Decimal> = HashMap::new();
    for p in places.iter().filter(|p| p.locked.is_some()) { *used.entry(&p.protocol_name).or_default() += p.amount }
    let by_apy = |descending: bool| {
        let mut v: Vec<&Place> = places.iter().collect();
        v.sort_by(|a, b| if descending { b.apy.cmp(&a.apy) } else { a.apy.cmp(&b.apy) });
        v
    };
    for p in by_apy(true) {
        if p.locked.is_some() { target.insert(&p.name, p.amount); continue }
        let room = (cap - used.get(p.protocol_name.as_str()).copied().unwrap_or_default()).max(Decimal::ZERO);
        let t = if p.closed.is_none() { room } else { room.min(p.amount) }.min(remaining);
        target.insert(&p.name, t);
        *used.entry(&p.protocol_name).or_default() += t;
        remaining -= t;
    }
    let target_of = |target: &HashMap<&str, Decimal>, name: &str| target.get(name).copied().unwrap_or_default();
    // What can't be placed under the cap stays in the worst places it is in now: they give up less.
    for p in by_apy(false) {
        if !(remaining > Decimal::ZERO && p.locked.is_none()) { continue }
        let back = remaining.min((p.amount - target_of(&target, &p.name)).max(Decimal::ZERO));
        *target.entry(&p.name).or_default() += back;
        remaining -= back;
    }
    for p in places {
        let Some(lock) = &p.locked else { continue };
        if places.iter().any(|q| q.locked.is_none() && q.closed.is_none() && q.apy > p.apy) {
            notes.push(format!("{} keeps {} USDC: {lock}.", p.name, fmt(p.amount)));
        }
    }
    // Worst money moves first, into the best places first.
    let mut excess: Vec<(&Place, Decimal)> = by_apy(false).into_iter()
        .filter(|p| p.amount - target_of(&target, &p.name) >= half).map(|p| (p, p.amount - target_of(&target, &p.name))).collect();
    let deficits: Vec<(&Place, Decimal)> = by_apy(true).into_iter()
        .filter(|p| target_of(&target, &p.name) - p.amount >= half).map(|p| (p, target_of(&target, &p.name) - p.amount)).collect();
    let (mut moves, mut skipped) = (Vec::new(), Vec::new());
    for (to, need) in deficits {
        let mut left = need;
        for (from, have) in excess.iter_mut() {
            if !(left > Decimal::ZERO && *have > Decimal::ZERO) { continue }
            let amount = left.min(*have);
            let step = Move { from: from.name.clone(), to: to.name.clone(), amount, from_apy: from.apy, to_apy: to.apy, cost: from.exit_cost + to.entry_cost };
            *have -= amount;
            left -= amount;
            if amount < min_move {
                skipped.push(Skip { reason: format!("{} USDC is too little to move", fmt(amount)), step });
            } else if step.payback_days().is_some_and(|days| days <= payback_days) {
                moves.push(step);
            } else {
                let days = step.payback_days().map_or_else(|| "never".into(), |d| format!("{} days", fmt(up(d))));
                let reason = format!("+${} a year against ≈ ${} to move: pays back in {days}, more than {}", fmt(step.yearly_gain()), fmt(step.cost), fmt(payback_days));
                skipped.push(Skip { step, reason });
            }
        }
    }
    if moves.is_empty() && skipped.is_empty() && notes.is_empty() {
        notes.push(format!("Already at the best rates within {}% per protocol: nothing to move.", fmt(max_share)));
    }
    for p in places {
        if p.amount > cap + half && p.locked.is_none() && !moves.iter().any(|m: &Move| m.from == p.name) {
            notes.push(format!("{} stays above {}% ({} of {} USDC): moving the rest out wouldn't pay for itself.", p.name, fmt(max_share), fmt(p.amount), fmt(total)));
        }
    }
    Plan { moves, skipped, notes }
}

/// `NSDecimalRound(…, 0, .up)`: toward +∞.
fn up(d: Decimal) -> Decimal { d.round_dp_with_strategy(0, RoundingStrategy::ToPositiveInfinity) }

/// Up to 2 places, en_US, never grouped.
fn fmt(d: Decimal) -> String { two_places(d, false) }

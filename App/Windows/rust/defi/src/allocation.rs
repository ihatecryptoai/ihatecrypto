//! "Spread it over the best rates" (`Allocation.swift`): the free amount goes to the best rates first, no protocol
//! above `max_share` of everything the asset is in (what is already deposited counts), and a protocol whose entry costs
//! more than it is worth is skipped with the reason. Pure, so the rules are tested apart from the chain.

use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::{text, two_places};

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    /// % a year.
    pub apy: Decimal,
    /// Already there, in the asset's units.
    pub existing: Decimal,
    /// Why ihatecrypto will not put money here now, or nil.
    pub blocked: Option<String>,
    /// The protocol the limit is counted per: «Aave» for every Aave market, whatever the network — the same
    /// contracts' risk, so spreading over networks doesn't get around it.
    pub protocol_name: String,
}

impl Place {
    /// Nothing there yet, open, its own protocol: `Place { existing, blocked, protocol_name, ..Place::new(…) }` for the rest.
    pub fn new(name: &str, apy: Decimal) -> Self {
        Self { name: name.into(), apy, existing: Decimal::ZERO, blocked: None, protocol_name: name.into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub place: String,
    pub amount: Decimal,
    pub apy: Decimal,
}

/// `Allocation.Result`.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub slices: Vec<Slice>,
    /// Not placed: nowhere left under the limit.
    pub leftover: Decimal,
    pub notes: Vec<String>,
}

impl Plan {
    /// Expected a year from the new slices, in the asset's units.
    pub fn yearly(&self) -> Decimal { self.slices.iter().map(|s| s.amount * s.apy / Decimal::ONE_HUNDRED).sum() }
}

/// `min_slice`: smaller pieces are not worth a transaction.
pub fn plan(free: Decimal, places: &[Place], max_share: Decimal, min_slice: Decimal) -> Plan {
    let total = free + places.iter().map(|p| p.existing).sum::<Decimal>();
    let cap = total * max_share / Decimal::ONE_HUNDRED;
    let mut left = free;
    let mut slices = Vec::new();
    let mut notes = Vec::new();
    // What each protocol holds, across its places, and grows by as slices are placed.
    let mut used: HashMap<&str, Decimal> = HashMap::new();
    for p in places { *used.entry(&p.protocol_name).or_default() += p.existing }
    let mut sorted: Vec<&Place> = places.iter().collect();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.apy));
    for p in sorted {
        if let Some(reason) = &p.blocked {
            notes.push(format!("{} ({}%): skipped, {reason}.", p.name, two_places(p.apy, true)));
            continue;
        }
        let held = used.get(p.protocol_name.as_str()).copied().unwrap_or_default();
        let room = (cap - held).max(Decimal::ZERO);
        let amount = left.min(room);
        if amount < min_slice {
            if room < min_slice && held > Decimal::ZERO {
                notes.push(format!("{}: {} is already at the {}% limit.", p.name, p.protocol_name, text(max_share)));
            }
            continue;
        }
        slices.push(Slice { place: p.name.clone(), amount, apy: p.apy });
        *used.entry(&p.protocol_name).or_default() += amount;
        left -= amount;
        if left < min_slice { break }
    }
    if left >= min_slice && (!slices.is_empty() || free >= min_slice) {
        notes.push(format!("The rest is not placed: the other protocols are at the {}% limit or unavailable. To raise the limit, add “max 70% per protocol” to the request.", text(max_share)));
    }
    Plan { slices, leftover: if left < min_slice { Decimal::ZERO } else { left }, notes }
}

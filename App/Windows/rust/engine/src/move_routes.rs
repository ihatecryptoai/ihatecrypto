//! A move's route on its block (MoveRoutes.swift): FROM, BRIDGE, SWAP, SIGN… with the balances each step changes.
//! Layer 2 adds here: quote_route, the route steps of every move, genuine_contracts, token names for the labels, and the
//! rest of the file's `extension Engine`.

use std::collections::{HashMap, HashSet};

/// Contracts' symbols looked up for a route's labels (the Mac's tokenNames), each asked once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContractNames {
    pub known: HashMap<String, String>,
    pub asked: HashSet<String>,
}

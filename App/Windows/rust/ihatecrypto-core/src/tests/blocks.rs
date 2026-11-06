use std::collections::HashSet;

use super::set;
use crate::blocks::{ALL, GROUPS};
use crate::normalize::ChatRouter;

/// BlocksTests.everyBlockExampleParses: «Add block» puts a block's example into the chat, so each must be a command the
/// agent's forms parse; every group listed, every label once.
#[test]
fn every_block_example_parses() {
    let markets = set(&["SOL", "ETH", "BTC", "xyz:TSLA"]);
    for block in ALL.iter().filter(|b| !b.automatic) {
        assert!(ChatRouter::parses(block.example, &markets), "{}: «{}»", block.label, block.example);
    }
    assert_eq!(ALL.iter().map(|b| b.group).collect::<HashSet<_>>(), GROUPS.into_iter().collect());
    assert_eq!(ALL.iter().map(|b| b.id()).collect::<HashSet<_>>().len(), ALL.len());
    // The ones the agent adds by itself have no example; every other one has (the engine's «/» list shows those).
    assert!(ALL.iter().all(|b| b.automatic == b.example.is_empty()));
}

//! The window's browser and its sites (Browser.swift): a site's request waiting for the person, what reviewing one
//! found, and the sites this run connected.
//! Layer 2 adds here: PhishingList (MetaMask's list in phishing.json, refreshed daily), dapp (each wallet method a page
//! calls), site_state, the EVM and Solana reviews and simulations, connecting and switching chains.

use std::collections::{HashMap, HashSet};

use tokio::sync::oneshot;

use crate::consent::{SiteBalance, SiteAskView, SiteTick};

/// A site's request waiting for the person (the Mac's SiteRequest): its own block on the canvas, apart from the
/// person's card, answered once by its id (consent.rs).
#[derive(Debug)]
pub struct SiteAsk {
    pub view: SiteAskView,
    answered: Option<oneshot::Sender<bool>>,
}

impl SiteAsk {
    pub fn new(view: SiteAskView, answered: oneshot::Sender<bool>) -> Self { SiteAsk { view, answered: Some(answered) } }

    /// The page gets its answer once; later answers change nothing.
    pub fn answer(&mut self, yes: bool) {
        if let Some(tx) = self.answered.take() { let _ = tx.send(yes); }
    }
}

/// A site's request refused, with the words the page gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteError(pub String);

impl std::fmt::Display for SiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for SiteError {}

/// What reviewing a site's transaction found, for its block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SiteReview {
    pub lines: Vec<String>,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
    pub problems: Vec<String>,
    pub recipients: Vec<String>,
    pub gas: u64,
    pub accounts: Vec<String>,
    pub token_owners: HashMap<String, String>,
    pub unknown_programs: Vec<String>,
    /// What needs the person's tick before Approve counts (consent.rs).
    pub ticks: Vec<SiteTick>,
    /// The card's lead, balances and folded checks (a Solana transaction, the Mac's sheet).
    pub lead: Option<String>,
    pub balances: Vec<SiteBalance>,
    pub checks: Vec<String>,
}

/// The sites connected in this run: their origins, by chain, and the EVM chain each switched to (Arbitrum by default).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConnectedSites {
    pub solana: HashSet<String>,
    pub evm: HashSet<String>,
    pub chain: HashMap<String, u64>,
}

/// The EVM calls a site may make that only read.
pub const READ_ONLY: [&str; 14] = [
    "eth_call", "eth_blockNumber", "eth_getBalance", "eth_getCode", "eth_getTransactionCount", "eth_getTransactionReceipt", "eth_getTransactionByHash",
    "eth_estimateGas", "eth_gasPrice", "eth_feeHistory", "eth_getBlockByNumber", "eth_getLogs", "eth_maxPriorityFeePerGas", "eth_getStorageAt",
];
/// The EVM networks a site may use, as the Mac's browser (Ethereum, Arbitrum, Base): where the balance simulation is
/// known to run, so a card never shows only «Unavailable».
pub const SITE_CHAINS: [u64; 3] = [1, 42161, 8453];
/// arb1 crashes on eth_simulateV1; publicnode runs it.
pub const SIMULATION_RPC: [(u64, &str); 1] = [(42161, "https://arbitrum-one-rpc.publicnode.com")];
/// Solana's own programs: one quiet line after the ones that do the work.
pub const SOLANA_PLUMBING: [&str; 5] = ["System", "Compute Budget", "SPL Token", "Associated Token", "Address Lookup Table"];
/// What the window's own part asks while the app is locked: the phishing check, which the Mac's browser makes whatever
/// the lock. The page beside the chat stays alive under the lock screen, and a check refused there would read as one
/// the window couldn't make, so nothing loads.
pub const BROWSER_WHILE_LOCKED: [&str; 1] = ["siteState"];

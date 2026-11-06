//! A cheap phishing check (`SiteCheck.swift`): the host against dApps the user is likely to mean. A host one or two
//! letters away from a known one is the classic fake («jup.ag» → «jupp.ag»).

use std::collections::HashSet;

use crate::foundation::chars;

pub const KNOWN: [&str; 15] = [
    "jup.ag", "raydium.io", "orca.so", "kamino.finance", "kamino.com", "drift.trade", "meteora.ag", "marginfi.com",
    "app.hyperliquid.xyz", "hyperliquid.xyz", "relay.link", "solscan.io", "phantom.app", "pump.fun", "tensor.trade",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Known,
    Lookalike(String),
    Unknown,
    Local,
}

pub fn check(host: &str) -> Verdict {
    let h = host.to_lowercase();
    if h.is_empty() { return Verdict::Local }
    if KNOWN.iter().any(|k| h == *k || h.ends_with(&format!(".{k}"))) { return Verdict::Known }
    let labels: Vec<&str> = h.split('.').filter(|l| !l.is_empty()).collect();
    let base = labels[labels.len().saturating_sub(2)..].join(".");
    // A known name as a part of a label anywhere else (jup.ag.claim-airdrop.com, hyperliquid-app.xyz, kamino-finance.app,
    // jup-ag.com) is the other classic fake, and so is any punycode label: letters from another alphabet that look like ours.
    let parts: HashSet<&str> = h.split(['.', '-']).filter(|p| !p.is_empty()).collect();
    if let Some(named) = KNOWN.iter().find(|k| parts.contains(brand(k))) { return Verdict::Lookalike(named.to_string()) }
    if labels.iter().any(|l| l.starts_with("xn--")) {
        return Verdict::Lookalike(KNOWN.iter().min_by_key(|k| distance(&base, k)).unwrap_or(&KNOWN[0]).to_string());
    }
    match KNOWN.iter().find(|k| distance(&base, k) <= 2 || distance(&h, k) <= 2) {
        Some(near) => Verdict::Lookalike(near.to_string()),
        None => Verdict::Unknown,
    }
}

/// The name a known site goes by: the label before its top-level domain («hyperliquid» of app.hyperliquid.xyz).
fn brand(domain: &str) -> &str {
    let labels: Vec<&str> = domain.split('.').filter(|l| !l.is_empty()).collect();
    labels.len().checked_sub(2).map_or("", |i| labels[i])
}

/// On MetaMask's phishing list (eth-phishing-detect): the host or any domain above it is blocked, unless the list's
/// allowlist names one of them («evil.github.io» is blocked by its own entry, not by github.io).
pub fn listed(host: &str, blocked: &HashSet<String>, allowed: &HashSet<String>) -> bool {
    let h = host.to_lowercase();
    let parts: Vec<&str> = h.split('.').filter(|l| !l.is_empty()).collect();
    let domains: Vec<String> = (0..parts.len().saturating_sub(1)).map(|i| parts[i..].join(".")).collect();
    !domains.iter().any(|d| allowed.contains(d)) && domains.iter().any(|d| blocked.contains(d))
}

/// Levenshtein's distance over Characters.
fn distance(a: &str, b: &str) -> usize {
    let (a, b) = (chars(a), chars(b));
    if a.is_empty() || b.is_empty() { return a.len().max(b.len()) }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = cur;
        }
    }
    row[b.len()]
}

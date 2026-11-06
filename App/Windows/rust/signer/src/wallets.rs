//! The wallets the signer holds, public parts only, in the trusted settings; the secrets are the vault's, one sealed file
//! each (`Wallets.swift`). Which wallet signs is the signer's own state. The active wallet's addresses are mirrored under
//! the keys a single wallet used («solana», «evm», «near»), so every check that reads «our address» reads the active one's.

use std::cell::RefCell;

use signer_api::SignerWallet;

use crate::trusted::{Pref, prefs};
use crate::vault::{self, seeds};

thread_local! {
    /// The automation wallet for one request (`asWallet`): while it's set, the key, «our addresses», the approved
    /// addresses and every per-wallet name follow it instead of the active wallet.
    static SIGNING: RefCell<Option<SignerWallet>> = const { RefCell::new(None) };
}

/// Runs `body` as `wallet` (the signer's `asWallet`), then back.
pub fn as_wallet<T>(wallet: SignerWallet, body: impl FnOnce() -> T) -> T {
    let before = SIGNING.with(|s| s.replace(Some(wallet)));
    let out = body();
    SIGNING.with(|s| *s.borrow_mut() = before);
    out
}

pub fn signing() -> Option<SignerWallet> { SIGNING.with(|s| s.borrow().clone()) }

/// The registry, migrated from the single seed on first read (wallet «1», «Main», active).
pub fn all() -> Vec<SignerWallet> {
    let stored = prefs::data("wallets").and_then(|d| serde_json::from_slice::<Vec<SignerWallet>>(&d).ok());
    let was_stored = stored.is_some();
    let list = SignerWallet::migrated(stored, seeds::first_present(), prefs::string("solana"), prefs::string("evm"), prefs::string("near"));
    if !was_stored && !list.is_empty() {
        save(&list);
        prefs::set("activeWallet", Some(Pref::String(SignerWallet::FIRST_ID.into())));
    }
    list
}

pub fn save(list: &[SignerWallet]) { prefs::set("wallets", serde_json::to_vec(list).ok().map(Pref::Data)) }

pub fn active() -> Option<SignerWallet> {
    if let Some(w) = signing() { return Some(w) }
    let all = all();
    SignerWallet::on(&all, prefs::string("activeWallet").as_deref()).cloned()
}

pub fn active_id() -> String { active().map_or_else(|| SignerWallet::FIRST_ID.into(), |w| w.id) }

/// A per-wallet name: wallet «1» keeps the one it had before there were several.
pub fn scoped(base: &str, id: &str) -> String { if id == SignerWallet::FIRST_ID { base.into() } else { format!("{base}-{id}") } }

/// Signing moves to `wallet` (None: none left): its addresses become «ours», and the next signature asks Windows Hello.
pub fn activate(wallet: Option<&SignerWallet>) {
    // Never the automation wallet, whoever asks: none instead.
    let wallet = wallet.filter(|w| !w.is_automation());
    prefs::set("activeWallet", wallet.map(|w| Pref::String(w.id.clone())));
    for (key, value) in [("solana", wallet.and_then(|w| w.solana.clone())), ("evm", wallet.and_then(|w| w.evm.clone())), ("near", wallet.and_then(|w| w.near.clone()))] {
        prefs::set(key, value.map(Pref::String));
    }
    vault::end_session();
}

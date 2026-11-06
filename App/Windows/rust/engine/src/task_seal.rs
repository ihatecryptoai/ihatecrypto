//! Scheduled tasks run by themselves from the automation wallet, and tasks.json is a file any program running as the
//! user can rewrite — the Mac's TaskSeal (the 2026-10-24 review). Each save is tagged with an HMAC; a list whose tag
//! doesn't match is loaded paused and said once. The key is made with the first list saved; a list found with no key
//! beside it had its key taken away, and loads paused too.
//! ponytail: the key is kept under DPAPI, which any program of the same Windows user can open and tag a list with (the
//! Mac's Keychain asks such a program first); such a program can read the automation wallet's seed as well (docs/windows.md
//! § Keys), and the signer still sends that wallet's money only to the user's own wallets and confirmed addresses. A key
//! the signer holds, tagging the list itself, would close it.

use hmac::{Hmac, Mac};
use ihatecrypto_core::scheduled_task::ScheduledTask;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::store;

/// The app's list and the key's name in the engine's folder: one key for every wallet's list; a test keeps its own.
pub const TASKS: &str = "tasks.json";
pub const KEY: &str = "task-seal.key";

/// The key and whether it was made just now (None: it couldn't be kept).
pub fn key(file: &str) -> Option<(Zeroizing<Vec<u8>>, bool)> {
    if let Some(raw) = std::fs::read(store::url(file)).ok().and_then(|sealed| store::unprotect(&sealed)) && raw.len() == 32 {
        return Some((raw, false));
    }
    let mut fresh = Zeroizing::new(vec![0u8; 32]);
    getrandom::fill(&mut fresh).ok()?;
    let sealed = store::protect(&fresh)?;
    store::write(file, &sealed).ok()?;
    Some((fresh, true))
}

pub fn tag(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// «tasks.json» → «tasks.seal», a wallet's «tasks-3.json» → «tasks-3.seal»: each list has its own tag, one key for all.
pub fn seal_name(file: &str) -> String { format!("{}.seal", file.rsplit_once('.').map_or(file, |(stem, _)| stem)) }

pub fn save(tasks: &[ScheduledTask], file: &str, key_file: &str) {
    let Ok(data) = serde_json::to_vec(tasks) else { return };
    let _ = store::write(file, &data);
    match key(key_file) {
        Some((key, _)) => { let _ = store::write(&seal_name(file), &tag(&data, &key)); }
        None => { let _ = std::fs::remove_file(store::url(&seal_name(file))); }
    }
}

/// The saved tasks, every one paused and without its Confirmed prices when their list was changed outside ihatecrypto;
/// and whether it was.
pub fn load(file: &str, key_file: &str) -> (Vec<ScheduledTask>, bool) {
    let Ok(data) = std::fs::read(store::url(file)) else { return (vec![], false) };
    let mut tasks: Vec<ScheduledTask> = serde_json::from_slice(&data).unwrap_or_default();
    // A list with no key beside it: the key was deleted, so any list would pass with the next one made.
    let intact = match key(key_file) {
        Some((key, false)) => std::fs::read(store::url(&seal_name(file))).ok() == Some(tag(&data, &key)),
        _ => tasks.is_empty(),
    };
    // The prices Confirmed in them too: the Schedule doesn't show them, so one written in could pass unseen.
    if !intact {
        for t in &mut tasks {
            t.paused = Some(true);
            t.prices = None;
        }
    }
    let tampered = !intact && !tasks.is_empty();
    (tasks, tampered)
}

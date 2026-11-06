//! What the signer trusts that isn't a secret — the wallets and their addresses, which one signs, the address book, the
//! approved and own addresses, the session length, the trading keys' switches — in `state.json`, sealed with an HMAC
//! under the Windows Hello key (`Trusted.swift`). Any program of the user can write the file, so the signer checks the
//! seal every time Windows Hello opens a key (`verify`), and a change made outside it stops every signature.
//!
//! Changes that only take trust away may be made with no key in hand: they go to `pending.json`, which is read only as
//! far as it takes trust away from the sealed state, and folded into the seal at the next prompt. Anything else asks
//! Windows Hello first. The files are the Swift signer's, byte for byte readable both ways.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use hmac::{Hmac, Mac};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::Sha256;
use signer_api::{SignerAddress, SignerWallet, session};
use wallet::{Result, WalletError, b64};

use crate::vault::{self, Key};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub prefs: BTreeMap<String, Pref>,
    pub recipients: BTreeMap<String, Vec<String>>,
    /// Contracts whose calls the signer can't read, allowed as calls only, per wallet (None in states sealed before
    /// there were two lists, so their seal still checks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<BTreeMap<String, Vec<String>>>,
    pub own: Vec<String>,
    pub book: Vec<SignerAddress>,
}

/// A setting's value. On the wire as Swift's synthesized Codable writes an enum: {"string": {"_0": "…"}}.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pref {
    String(String),
    Bool(bool),
    Int(i64),
    Data(Vec<u8>),
}

#[derive(Serialize, Deserialize)]
struct Wrapped<T> {
    #[serde(rename = "_0")]
    value: T,
}

#[derive(Serialize, Deserialize)]
struct Bytes(#[serde(with = "b64")] Vec<u8>);

impl Serialize for Pref {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = BTreeMap::new();
        let v = match self {
            Pref::String(v) => ("string", serde_json::to_value(Wrapped { value: v })),
            Pref::Bool(v) => ("bool", serde_json::to_value(Wrapped { value: v })),
            Pref::Int(v) => ("int", serde_json::to_value(Wrapped { value: v })),
            Pref::Data(v) => ("data", serde_json::to_value(Wrapped { value: Bytes(v.clone()) })),
        };
        map.insert(v.0, v.1.map_err(serde::ser::Error::custom)?);
        map.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Pref {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let map = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
        let [(kind, value)]: [(String, serde_json::Value); 1] = map.into_iter().collect::<Vec<_>>().try_into()
            .map_err(|_| D::Error::custom("a setting is one case"))?;
        let get = |v: serde_json::Value| v.get("_0").cloned().ok_or_else(|| D::Error::missing_field("_0"));
        Ok(match kind.as_str() {
            "string" => Pref::String(serde_json::from_value(get(value)?).map_err(D::Error::custom)?),
            "bool" => Pref::Bool(serde_json::from_value(get(value)?).map_err(D::Error::custom)?),
            "int" => Pref::Int(serde_json::from_value(get(value)?).map_err(D::Error::custom)?),
            "data" => Pref::Data(serde_json::from_value::<Bytes>(get(value)?).map_err(D::Error::custom)?.0),
            other => return Err(D::Error::unknown_variant(other, &["string", "bool", "int", "data"])),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    #[serde(with = "b64")]
    body: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_b64")]
    mac: Option<Vec<u8>>,
}

mod opt_b64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v { Some(b) => wallet::b64::serialize(b, s), None => s.serialize_none() }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        use base64::Engine;
        Option::<String>::deserialize(d)?.map(|s| base64::engine::general_purpose::STANDARD.decode(s).map_err(serde::de::Error::custom)).transpose()
    }
}

/// (sealed, effective) as last read; None: read again.
static LOADED: Mutex<Option<(State, State)>> = Mutex::new(None);

fn seal(body: &[u8], key: &Key) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&*vault::subkey(key, "state")).expect("any key");
    mac.update(body);
    mac.finalize().into_bytes().to_vec()
}

fn decode(name: &str) -> Option<(File, State)> {
    let file: File = serde_json::from_slice(&vault::read(name)?).ok()?;
    let state = serde_json::from_slice(&file.body).ok()?;
    Some((file, state))
}

fn load(loaded: &mut Option<(State, State)>) -> (State, State) {
    if let Some(l) = loaded { return l.clone() }
    let sealed = decode("state.json").map(|d| d.1).unwrap_or_default();
    let mut effective = sealed.clone();
    if let Some((_, pending)) = decode("pending.json") {
        if takes_trust_away(&sealed, &pending) { effective = pending }
    }
    *loaded = Some((sealed.clone(), effective.clone()));
    (sealed, effective)
}

fn write(loaded: &mut Option<(State, State)>, state: &State, key: Option<&Key>) -> Result<()> {
    let body = serde_json::to_vec(state).map_err(|e| WalletError(e.to_string()))?;
    let file = File { mac: key.map(|k| seal(&body, k)), body };
    vault::write("state.json", &serde_json::to_vec(&file).map_err(|e| WalletError(e.to_string()))?)?;
    vault::remove("pending.json");
    *loaded = Some((state.clone(), state.clone()));
    Ok(())
}

pub fn state() -> State { load(&mut LOADED.lock().expect("trusted")).1 }

pub fn tampered() -> WalletError {
    WalletError("The signer's protected settings — the wallets, the address book, the approved addresses — were changed outside it, by a program on this computer or a damaged file. Nothing was signed. Check this computer for malware. The recovery phrases restore your wallets on a clean one.".into())
}

/// Checks the sealed state with the key Windows Hello just gave. `fresh`: that key was made just now, so nothing was
/// sealed before: only what such a time can hold is kept — the wallets whose keys are here; no approved, own or saved
/// addresses (each of those needs Windows Hello, which would have made the key).
pub fn verify(key: &Key, fresh: bool) -> Result<()> {
    let mut loaded = LOADED.lock().expect("trusted");
    // What this process has decided by so far must be what the seal holds, or a file changed and changed back between
    // the decision and this check would pass.
    let used = loaded.take().map(|l| l.1);
    if fresh {
        let mut state = decode("state.json").map(|d| d.1).unwrap_or_default();
        if let Some(Pref::Data(list)) = state.prefs.get("wallets") {
            if let Ok(wallets) = serde_json::from_slice::<Vec<SignerWallet>>(list) {
                let kept: Vec<SignerWallet> = wallets.into_iter().filter(|w| vault::exists(&vault::seed_file(&w.id))).collect();
                match serde_json::to_vec(&kept) {
                    Ok(d) => { state.prefs.insert("wallets".into(), Pref::Data(d)); }
                    Err(_) => { state.prefs.remove("wallets"); }
                }
                let active_kept = matches!(state.prefs.get("activeWallet"), Some(Pref::String(a)) if kept.iter().any(|w| &w.id == a));
                if matches!(state.prefs.get("activeWallet"), Some(Pref::String(_))) && !active_kept {
                    for k in ["activeWallet", "solana", "evm", "near"] { state.prefs.remove(k); }
                }
            }
        }
        (state.recipients, state.calls, state.own, state.book) = (BTreeMap::new(), None, vec![], vec![]);
        return write(&mut loaded, &state, Some(key));
    }
    let Some((file, sealed)) = decode("state.json") else {
        if vault::exists("state.json") { return Err(tampered()) }
        // Never written since the key was made: only a crash right after making it leaves this.
        return write(&mut loaded, &State::default(), Some(key));
    };
    if file.mac.as_deref() != Some(seal(&file.body, key).as_slice()) { return Err(tampered()) }
    let mut effective = sealed.clone();
    if let Some((_, pending)) = decode("pending.json") {
        if pending != sealed && takes_trust_away(&sealed, &pending) { effective = pending }
    }
    if used.is_some_and(|u| u != effective) { return Err(tampered()) }
    // What was changed with no key in hand is sealed now, as far as it only takes trust away.
    if effective != sealed || vault::exists("pending.json") { return write(&mut loaded, &effective, Some(key)) }
    *loaded = Some((sealed.clone(), sealed));
    Ok(())
}

/// Changes the trusted state. With a key in hand (this request's, or an open session's) or before any wallet needs
/// Windows Hello, it's sealed at once; a change that only takes trust away waits in `pending.json`; anything else asks
/// Windows Hello first (`reason`; None: it isn't made).
pub fn mutate(reason: Option<&str>, change: impl FnOnce(&mut State)) -> Result<()> {
    let (before, current, next) = {
        let mut loaded = LOADED.lock().expect("trusted");
        let (sealed, effective) = load(&mut loaded);
        let mut next = effective.clone();
        change(&mut next);
        (sealed, effective, next)
    };
    if next == current { return Ok(()) }
    if let Some(key) = vault::current_key() { return write(&mut LOADED.lock().expect("trusted"), &next, Some(&key)) }
    if vault::master().is_none() { return write(&mut LOADED.lock().expect("trusted"), &next, None) }
    if takes_trust_away(&before, &next) {
        let body = serde_json::to_vec(&next).map_err(|e| WalletError(e.to_string()))?;
        vault::write("pending.json", &serde_json::to_vec(&File { body, mac: None }).map_err(|e| WalletError(e.to_string()))?)?;
        *LOADED.lock().expect("trusted") = Some((before, next));
        return Ok(());
    }
    let Some(reason) = reason else { return Ok(()) };
    vault::confirm(reason, None)?;
    let Some(key) = vault::current_key() else { return Err(WalletError("Not confirmed: nothing was changed.".into())) };
    write(&mut LOADED.lock().expect("trusted"), &next, Some(&key))
}

/// `new` gives nothing `old` didn't: wallets only renamed or gone, addresses only gone, trading keys only paused, a
/// session no longer — and every other setting the same.
pub fn takes_trust_away(old: &State, new: &State) -> bool {
    let subset = |a: &[String], b: Option<&Vec<String>>| {
        let b: HashSet<&String> = b.map(|b| b.iter().collect()).unwrap_or_default();
        a.iter().all(|x| b.contains(x))
    };
    if new.recipients.iter().any(|(id, list)| !subset(list, old.recipients.get(id))) { return false }
    if new.calls.iter().flatten().any(|(id, list)| !subset(list, old.calls.as_ref().and_then(|c| c.get(id)))) { return false }
    if !subset(&new.own, Some(&old.own)) { return false }
    let book: HashSet<&SignerAddress> = old.book.iter().collect();
    if !new.book.iter().all(|a| book.contains(a)) { return false }
    let keys: std::collections::BTreeSet<&String> = old.prefs.keys().chain(new.prefs.keys()).collect();
    for key in keys {
        let (was, now) = (old.prefs.get(key), new.prefs.get(key));
        if was == now { continue }
        match key.as_str() {
            "wallets" => {
                let list = |p: Option<&Pref>| match p {
                    Some(Pref::Data(d)) => serde_json::from_slice::<Vec<SignerWallet>>(d).unwrap_or_default(),
                    _ => vec![],
                };
                let before = list(was);
                for w in list(now) {
                    let Some(m) = before.iter().find(|b| b.id == w.id) else { return false };
                    let mut m = m.clone();
                    m.label = w.label.clone();
                    if m != w { return false }
                }
            }
            "sessionLength" => {
                let seconds = |p: Option<&Pref>| if let Some(Pref::Int(v)) = p { *v } else { session::STANDARD };
                if seconds(now) > seconds(was) { return false }
            }
            k if k.starts_with("agentActive") => {
                if !(now.is_none() || now == Some(&Pref::Bool(false)) || now == Some(&Pref::String("paused".into()))) { return false }
            }
            _ => return false,
        }
    }
    true
}

/// For tests: forget what was read, as a new signer process would.
pub fn reload() { *LOADED.lock().expect("trusted") = None }

/// The Mac's SignerPrefs, on Windows: the same calls over the trusted state.
pub mod prefs {
    use super::*;

    pub fn get(key: &str) -> Option<Pref> { state().prefs.get(key).cloned() }
    pub fn string(key: &str) -> Option<String> { match get(key) { Some(Pref::String(s)) => Some(s), _ => None } }
    pub fn data(key: &str) -> Option<Vec<u8>> { match get(key) { Some(Pref::Data(d)) => Some(d), _ => None } }
    pub fn bool(key: &str) -> bool { matches!(get(key), Some(Pref::Bool(true))) }
    pub fn integer(key: &str) -> i64 { match get(key) { Some(Pref::Int(i)) => i, _ => 0 } }

    /// None removes the key. A change that gives trust with no key in hand asks Windows Hello first; turned down, the
    /// change isn't made.
    pub fn set(key: &str, value: Option<Pref>) {
        if let Err(e) = mutate(Some(&format!("change what the signer trusts: {key}")), |s| match value {
            Some(v) => { s.prefs.insert(key.into(), v); }
            None => { s.prefs.remove(key); }
        }) {
            eprintln!("ihatecrypto signer: {key} not changed: {e}");
        }
    }

    pub fn remove(key: &str) { set(key, None) }
}

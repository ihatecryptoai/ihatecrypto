//! The signer's files on Windows, in %LOCALAPPDATA%\ihatecrypto\signer, and the Windows Hello key that guards them
//! (`Vault.swift`). A wallet's seed is sealed with AES-GCM under a key derived from a Windows Hello signature (the same
//! for the same challenge, so it comes back only with the person — KeePassXC's way), then encrypted to the Windows user
//! (DPAPI). Every key that opens a seed comes from one signature (`master`), so one Windows Hello prompt covers one
//! move, as Touch ID does on a Mac. A test or automation wallet, made to sign with no one there, is under DPAPI only.
//! The files are the Swift signer's: either reads what the other wrote.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use signer_api::{SignerWallet, passcode, session};
use wallet::{Result, WalletError, b64};
use zeroize::Zeroizing;

use crate::platform::{self, Status};
use crate::trusted::{self, Pref, prefs};
use crate::wallets;

/// A 32-byte symmetric key, wiped when dropped.
pub type Key = Zeroizing<[u8; 32]>;

fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

#[cfg(debug_assertions)]
pub const PRODUCT: &str = "ihatecrypto-dev";
#[cfg(not(debug_assertions))]
pub const PRODUCT: &str = "ihatecrypto";

/// The Windows Hello key: one per Windows user and copy of the app.
pub fn hello_key() -> String { format!("{PRODUCT}-signer") }

/// What the person's presence is called in texts.
pub const PRESENCE: &str = "Windows Hello";

pub static FOLDER: LazyLock<PathBuf> = LazyLock::new(|| {
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("IHATECRYPTO_SIGNER_DIR") { return made(PathBuf::from(dir)) }
    // Off Windows only the stand-in for Windows Hello runs (tests, the UI on a Mac): never beside a real wallet.
    let base = if cfg!(windows) { std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir()) } else { std::env::temp_dir() };
    made(base.join(PRODUCT).join("signer"))
});

fn made(p: PathBuf) -> PathBuf {
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn path(name: &str) -> PathBuf { FOLDER.join(name) }
pub fn read(name: &str) -> Option<Vec<u8>> { std::fs::read(path(name)).ok() }
pub fn exists(name: &str) -> bool { path(name).exists() }
pub fn remove(name: &str) { let _ = std::fs::remove_file(path(name)); }

/// Written whole or not at all: a temporary file renamed over the old one.
pub fn write(name: &str, data: &[u8]) -> Result<()> {
    let tmp = path(&format!(".{name}.tmp"));
    std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, path(name))).map_err(|e| WalletError(format!("The signer couldn't write {name}: {e}")))
}

pub fn files() -> Vec<String> {
    std::fs::read_dir(&*FOLDER).map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect()).unwrap_or_default()
}

// MARK: DPAPI

pub fn protect(data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    platform::protect(data).ok_or_else(|| WalletError("Windows didn't encrypt the signer's file.".into()))
}

pub fn unprotect(data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    platform::unprotect(data).ok_or_else(|| WalletError("Windows didn't open the signer's file: it belongs to another Windows user or was damaged.".into()))
}

// MARK: Windows Hello

/// The person's Windows Hello signature of `challenge`, with `reason` in the signer's window meanwhile. `within`: the
/// prompt closes by itself after that many seconds unanswered (a scheduled move, as a Mac's Touch ID prompt closes
/// after a minute), so no one away leaves it up with every other request waiting behind it.
pub fn hello_sign(challenge: &[u8], reason: &str, create: bool, within: Option<u64>) -> Result<Zeroizing<Vec<u8>>> {
    let ms = within.map_or(0, |s| (s.clamp(1, 86_400) * 1000) as u32);
    let started = Instant::now();
    match platform::hello_sign(&hello_key(), challenge, reason, create, ms) {
        Ok(signature) => Ok(signature),
        Err(Status::Cancelled) => match within {
            Some(w) if started.elapsed().as_secs() + 1 >= w => fail(format!("No one answered Windows Hello within {w} seconds, so it closed: nothing was signed.")),
            _ => fail("Cancelled, or Windows Hello didn't match: nothing was signed."),
        },
        Err(Status::NotFound) => fail("The Windows Hello key that seals your wallets is gone (Windows was reset, or its PIN removed): add the wallets again from their recovery phrases."),
        Err(Status::Unavailable) => fail("Set up Windows Hello first (Settings › Accounts › Sign-in options: a PIN at least): it guards every wallet that holds money."),
        Err(Status::Locked) => fail("Windows Hello is locked after too many tries: unlock Windows with your PIN, then try again."),
        Err(Status::Failed) => fail("Windows Hello didn't answer: nothing was signed."),
    }
}

/// The challenge Windows Hello signs and a check of the key it gives, kept with every sealed seed too, so a lost
/// `master.json` costs nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Master {
    #[serde(with = "b64")]
    pub challenge: Vec<u8>,
    #[serde(with = "b64")]
    pub check: Vec<u8>,
}

pub fn master() -> Option<Master> {
    if let Some(m) = read("master.json").and_then(|d| serde_json::from_slice(&d).ok()) { return Some(m) }
    files().iter().filter(|f| f.ends_with(".seed")).find_map(|f| {
        let s = seeds::sealed(f).ok()??;
        Some(Master { challenge: s.challenge.clone()?, check: s.check.clone()? })
    })
}

fn hkdf(ikm: &[u8], salt: Option<&[u8]>, info: &[u8]) -> Key {
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(salt, ikm).expand(info, &mut *out).expect("32 bytes");
    out
}

pub fn master_key(signature: &[u8], challenge: &[u8]) -> Key { hkdf(signature, Some(challenge), b"ihatecrypto master") }

pub fn check_value(key: &Key) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&**key).expect("any key");
    mac.update(b"ihatecrypto check");
    mac.finalize().into_bytes().to_vec()
}

/// A key for one purpose («seed 1», «state»), from the master key.
pub fn subkey(master: &Key, label: &str) -> Key { hkdf(&**master, None, label.as_bytes()) }

fn random<const N: usize>() -> Zeroizing<[u8; N]> {
    let mut b = Zeroizing::new([0u8; N]);
    getrandom::fill(&mut *b).expect("the system's randomness");
    b
}

/// Windows Hello now, whatever session is open: the master key it gives. The first time (`fresh`), it makes the key in
/// Windows Hello (one prompt) and signs (another).
pub fn unlock(reason: &str, within: Option<u64>) -> Result<(Key, bool)> {
    if let Some(m) = master() {
        let signature = hello_sign(&m.challenge, reason, false, within)?;
        let key = master_key(&signature, &m.challenge);
        if check_value(&key) != m.check {
            return fail("Windows Hello answered with another key than the one that sealed your wallets: nothing was opened.");
        }
        return Ok((key, false));
    }
    let challenge = random::<32>();
    let signature = hello_sign(&*challenge, reason, true, within)?;
    let key = master_key(&signature, &*challenge);
    let m = Master { challenge: challenge.to_vec(), check: check_value(&key) };
    write("master.json", &serde_json::to_vec(&m).expect("json"))?;
    Ok((key, true))
}

// MARK: Sessions

thread_local! {
    /// What one request has opened: the trusted settings it changes are sealed with it even after `end_session`, which
    /// only stops the next request from reusing it. The host clears it before each request.
    static SCOPE: RefCell<Option<Key>> = const { RefCell::new(None) };
}

static SESSION: Mutex<Option<(Key, Instant)>> = Mutex::new(None);

/// A new request: nothing opened yet.
pub fn begin_request() { SCOPE.with(|s| *s.borrow_mut() = None) }

fn set_scope(key: &Key) { SCOPE.with(|s| *s.borrow_mut() = Some(key.clone())) }

/// The key of this request or of a session still open; None when it would take a prompt.
pub fn current_key() -> Option<Key> {
    if let Some(k) = SCOPE.with(|s| s.borrow().clone()) { return Some(k) }
    let session = SESSION.lock().expect("session");
    session.as_ref().filter(|(_, since)| since.elapsed() < Duration::from_secs(seeds::session_length() as u64)).map(|(k, _)| k.clone())
}

/// The master key for a move: the open session's, or Windows Hello with `reason`. The trusted settings are checked
/// against it before it is handed out: a key never serves settings changed outside the signer.
pub fn key(reason: &str, use_session: bool, within: Option<u64>) -> Result<Key> {
    if use_session {
        if let Some(key) = current_key() {
            // The file may have changed since the session opened: checked again before the key serves.
            trusted::verify(&key, false)?;
            set_scope(&key);
            return Ok(key);
        }
    }
    let (key, fresh) = unlock(reason, within)?;
    trusted::verify(&key, fresh)?;
    if use_session { *SESSION.lock().expect("session") = Some((key.clone(), Instant::now())) }
    set_scope(&key);
    agents::open_all(&key);
    Ok(key)
}

/// Windows Hello again, always: a fresh confirmation, whose key then serves the signature that follows.
pub fn confirm(reason: &str, within: Option<u64>) -> Result<()> {
    let (key, fresh) = unlock(reason, within)?;
    trusted::verify(&key, fresh)?;
    *SESSION.lock().expect("session") = Some((key.clone(), Instant::now()));
    set_scope(&key);
    agents::open_all(&key);
    Ok(())
}

pub fn end_session() { *SESSION.lock().expect("session") = None }

fn seal(key: &Key, plain: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let nonce = random::<12>();
    let cipher = Aes256Gcm::new_from_slice(&**key).expect("32 bytes");
    let sealed = cipher.encrypt(&Nonce::from(*nonce), Payload { msg: plain, aad }).map_err(|_| WalletError("Couldn't seal the key.".into()))?;
    Ok([nonce.to_vec(), sealed].concat()) // CryptoKit's combined: nonce ‖ ciphertext ‖ tag
}

fn open(key: &Key, combined: &[u8], aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if combined.len() < 28 { return None }
    let cipher = Aes256Gcm::new_from_slice(&**key).expect("32 bytes");
    cipher.decrypt(&Nonce::from(<[u8; 12]>::try_from(&combined[..12]).ok()?), Payload { msg: &combined[12..], aad }).ok().map(Zeroizing::new)
}

pub fn seed_file(id: &str) -> String { format!("wallet-{id}.seed") }

/// The Mac's SeedStore, on Windows: the same calls the signer makes, over the vault's files.
pub mod seeds {
    use super::*;

    /// A seed as it is kept: inside DPAPI, `box` is AES-GCM under the Windows Hello key (`presence`), or the seed itself
    /// for a wallet that signs with no prompt.
    #[derive(Serialize, Deserialize)]
    pub struct Sealed {
        pub version: i64,
        pub presence: bool,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "opt")]
        pub challenge: Option<Vec<u8>>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "opt")]
        pub check: Option<Vec<u8>>,
        #[serde(rename = "box", with = "b64")]
        pub sealed: Vec<u8>,
    }

    impl Drop for Sealed {
        fn drop(&mut self) { zeroize::Zeroize::zeroize(&mut self.sealed) }
    }

    pub(crate) mod opt {
        use serde::{Deserialize, Deserializer, Serializer};
        pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
            match v { Some(b) => wallet::b64::serialize(b, s), None => s.serialize_none() }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
            use base64::Engine;
            Option::<String>::deserialize(d)?.map(|s| base64::engine::general_purpose::STANDARD.decode(s).map_err(serde::de::Error::custom)).transpose()
        }
    }

    pub fn sealed(name: &str) -> Result<Option<Sealed>> {
        let Some(file) = read(name) else { return Ok(None) };
        let json = unprotect(&file)?;
        serde_json::from_slice(&json).map(Some).map_err(|e| WalletError(e.to_string()))
    }

    pub fn first_present() -> bool { exists(&seed_file(SignerWallet::FIRST_ID)) }

    pub fn session_length() -> i64 {
        let chosen = prefs::integer("sessionLength");
        if session::CHOICES.contains(&chosen) { chosen } else { session::STANDARD }
    }

    /// Seals the seed. Release builds ask Windows Hello for it except for a test wallet, as a Mac asks Touch ID.
    pub fn save(seed: &[u8], require_presence: bool, id: &str, test: bool) -> Result<()> {
        let presence = if cfg!(debug_assertions) { require_presence } else { require_presence || !test };
        let sealed = if presence {
            let key = super::key("keep this wallet's key under Windows Hello: it opens only with you", true, None)?;
            let Some(m) = master() else { return fail("Couldn't seal the key.") };
            Sealed { version: 1, presence: true, challenge: Some(m.challenge), check: Some(m.check), sealed: seal(&subkey(&key, &format!("seed {id}")), seed, &[])? }
        } else {
            Sealed { version: 1, presence: false, challenge: None, check: None, sealed: seed.to_vec() }
        };
        let json = Zeroizing::new(serde_json::to_vec(&sealed).expect("json"));
        write(&seed_file(id), &protect(&json)?)?;
        end_session();
        Ok(())
    }

    /// The seed, after Windows Hello or within its session. `use_session: false` (the automation wallet's) never reuses
    /// a session nor leaves one; that wallet opens with no prompt anyway.
    pub fn load(reason: &str, id: &str, use_session: bool) -> Result<Zeroizing<Vec<u8>>> {
        let Some(s) = sealed(&seed_file(id))? else { return fail("No wallet imported.") };
        if !s.presence { return Ok(Zeroizing::new(s.sealed.clone())) }
        let key = super::key(reason, use_session, None)?;
        open(&subkey(&key, &format!("seed {id}")), &s.sealed, &[])
            .ok_or_else(|| WalletError("This wallet's sealed key didn't open with Windows Hello's: nothing was signed.".into()))
    }

    /// A wallet's seed with no prompt, or None: only a test or the automation wallet opens so.
    pub fn without_prompt(id: &str) -> Option<Zeroizing<Vec<u8>>> {
        sealed(&seed_file(id)).ok()?.filter(|s| !s.presence).map(|s| Zeroizing::new(s.sealed.clone()))
    }

    pub fn delete(id: &str) {
        end_session();
        remove(&seed_file(id));
    }
}

/// The app's 6-digit passcode, as the Mac's: it opens the lock screen. Windows Hello signs every move here, so the code
/// never does. Kept in code.json under DPAPI: a salt, the code stretched with it (PBKDF2-HMAC-SHA256) and the wrong tries
/// in a row. A try is counted before it is checked; the fifth wrong one in a row deletes it all.
/// ponytail: a program of this Windows user can open code.json (DPAPI is the user's) and guess the 10⁶ codes offline —
/// the code hides the window from a person at the PC, as on a Mac; a TPM key with a PIN if it must stand against that.
pub mod code {
    use super::*;

    pub const TRIES: i64 = 5;
    pub const ROUNDS: u32 = 20_000;
    /// What opens the app without the code.
    pub const FALLBACK: &str = PRESENCE;
    /// Windows Hello always asks before a signature: a code set here only opens the app.
    pub const CAN_SIGN: bool = false;

    #[derive(Debug, PartialEq, Eq)]
    pub enum Entered { Right, Wrong { left: i64 }, Off, Uncounted }

    #[derive(Serialize, Deserialize)]
    struct Record {
        #[serde(with = "b64")]
        salt: Vec<u8>,
        #[serde(with = "b64")]
        hash: Vec<u8>,
        failures: i64,
    }

    /// One code operation at a time, so none sees another half done.
    static ENTERING: Mutex<()> = Mutex::new(());

    fn record() -> Option<Record> { serde_json::from_slice(&unprotect(&read("code.json")?).ok()?).ok() }
    fn save(r: &Record) -> bool { protect(&serde_json::to_vec(r).expect("json")).and_then(|d| write("code.json", &d)).is_ok() }

    /// A damaged or unreadable file counts as no code: the lock then opens only with Windows Hello, never without it.
    pub fn is_set() -> bool { record().is_some() }

    pub fn stretch(code: &str, salt: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(code.as_bytes(), salt, ROUNDS, &mut out);
        out
    }

    /// A new code in place of any earlier one (the signer asked Windows Hello first).
    pub fn set(code: &str) -> Result<()> {
        let _held = ENTERING.lock().expect("code");
        let salt = random::<16>().to_vec();
        if !save(&Record { hash: stretch(code, &salt).to_vec(), salt, failures: 0 }) { return fail("Windows didn't save the passcode.") }
        Ok(())
    }

    pub fn enter(code: &str) -> Entered {
        let _held = ENTERING.lock().expect("code");
        let Some(mut r) = record().filter(|r| r.failures < TRIES) else { remove("code.json"); return Entered::Off };
        r.failures += 1;
        if !save(&r) { return Entered::Uncounted }
        let typed = if passcode::problem(code).is_none() { stretch(code, &r.salt).to_vec() } else { vec![] };
        if typed.len() == r.hash.len() && typed.iter().zip(&r.hash).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0 {
            r.failures = 0;
            save(&r);
            return Entered::Right;
        }
        if r.failures >= TRIES { remove("code.json"); return Entered::Off }
        Entered::Wrong { left: TRIES - r.failures }
    }

    pub fn clear() {
        let _held = ENTERING.lock().expect("code");
        remove("code.json");
    }
}

/// Addresses outside the wallet the person allowed money to go to, per wallet, each confirmed in person once: money
/// recipients, and contracts whose calls the signer can't read, allowed as calls only.
pub mod recipients {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum List { Money, Calls }

    pub fn all(list: List) -> std::collections::HashSet<String> {
        let id = wallets::active_id();
        let s = trusted::state();
        let found = match list { List::Calls => s.calls.and_then(|c| c.get(&id).cloned()), List::Money => s.recipients.get(&id).cloned() };
        found.unwrap_or_default().into_iter().collect()
    }

    pub fn add(addresses: &[String], list: List) -> Result<()> {
        let id = wallets::active_id();
        let new: Vec<String> = addresses.iter().map(|a| key(a)).collect();
        let reason = if list == List::Calls { "allow a contract call ihatecrypto can't read" } else { "allow money to go to a new address" };
        trusted::mutate(Some(reason), |s| {
            let merge = |old: Option<&Vec<String>>| {
                let mut all: std::collections::BTreeSet<String> = old.cloned().unwrap_or_default().into_iter().collect();
                all.extend(new.iter().cloned());
                all.into_iter().collect::<Vec<_>>()
            };
            if list == List::Calls {
                let mut calls = s.calls.clone().unwrap_or_default();
                let merged = merge(calls.get(&id));
                calls.insert(id.clone(), merged);
                s.calls = Some(calls);
            } else {
                let merged = merge(s.recipients.get(&id));
                s.recipients.insert(id.clone(), merged);
            }
        })
    }

    /// EVM addresses compare without case; Solana's base58 is case-sensitive.
    pub fn key(address: &str) -> String { if address.starts_with("0x") { address.to_lowercase() } else { address.into() } }

    pub fn delete(id: &str) {
        let _ = trusted::mutate(Some("forget a wallet's approved addresses"), |s| {
            s.recipients.remove(id);
            if let Some(c) = s.calls.as_mut() { c.remove(id); }
        });
    }
}

/// The addresses of the wallets whose keys this signer holds, written when each key was derived here.
pub mod own {
    use super::*;

    pub fn all() -> std::collections::HashSet<String> { trusted::state().own.into_iter().collect() }

    /// Best effort, as on a Mac: without a key in hand (a test wallet signing) it's left for the next time.
    pub fn add(addresses: &[String]) {
        let have = all();
        let new: Vec<String> = addresses.iter().map(|a| recipients::key(a)).filter(|a| !have.contains(a)).collect();
        if new.is_empty() || (current_key().is_none() && master().is_some()) { return }
        let _ = trusted::mutate(None, |s| {
            let mut all: std::collections::BTreeSet<String> = s.own.iter().cloned().collect();
            all.extend(new);
            s.own = all.into_iter().collect();
        });
    }

    pub fn remove(addresses: &[String]) {
        let gone: std::collections::HashSet<String> = addresses.iter().map(|a| recipients::key(a)).collect();
        let _ = trusted::mutate(None, |s| s.own.retain(|a| !gone.contains(a)));
    }
}

/// The address book's names and addresses: changed only after Windows Hello with every address shown.
pub mod book {
    use super::*;
    use signer_api::SignerAddress;

    pub fn all() -> Vec<SignerAddress> { trusted::state().book }
    pub fn save(book: Vec<SignerAddress>) -> Result<()> { trusted::mutate(Some("save to your address book"), |s| s.book = book) }
}

/// The Hyperliquid trading key, sealed as a seed is — AES-GCM under the Windows Hello key — and opened once per signer
/// run, with the first Windows Hello the signer asks for anything or at the first order; from then on kept only in the
/// signer's memory. Its address sits beside it, so Settings shows the key without opening it. A wallet that signs with
/// no one there (test, automation) keeps it under DPAPI only, as it keeps its seed.
pub mod agents {
    use super::*;
    use std::collections::HashMap;

    #[derive(Serialize, Deserialize)]
    struct Sealed {
        version: i64,
        address: String,
        /// AES-GCM of the key under the Windows Hello key's subkey for this name, the address bound to it.
        #[serde(rename = "box", with = "b64")]
        sealed: Vec<u8>,
    }

    pub fn name(id: &str) -> String { format!("agent-{}", wallets::scoped("hyperliquid", id)) }
    /// Sealed under Windows Hello.
    pub fn file(id: &str) -> String { format!("{}.sealed", name(id)) }
    /// DPAPI only: a wallet that signs with no prompt. For a wallet whose keys are sealed it is never read.
    pub fn plain(id: &str) -> String { format!("{}.key", name(id)) }
    /// Set in the sealed trusted state once a key of `id` is sealed: a DPAPI-only file for it is never taken again.
    pub fn record(id: &str) -> String { format!("{}-sealed", name(id)) }

    static OPENED: LazyLock<Mutex<HashMap<String, Zeroizing<Vec<u8>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    /// When opening the key last went unanswered (or was declined). For ten minutes after, the key doesn't ask again.
    static UNANSWERED_AT: Mutex<Option<(Instant, chrono::DateTime<chrono::Local>)>> = Mutex::new(None);
    pub const QUIET_FOR: u64 = 600;

    /// An order that opens or adds comes from a card the person just confirmed: it may ask at once.
    pub fn ask_again() { *UNANSWERED_AT.lock().expect("agents") = None }

    fn asks_now() -> bool { UNANSWERED_AT.lock().expect("agents").is_none_or(|(at, _)| at.elapsed().as_secs() >= QUIET_FOR) }

    fn unanswered() -> Option<Zeroizing<Vec<u8>>> {
        *UNANSWERED_AT.lock().expect("agents") = Some((Instant::now(), chrono::Local::now()));
        None
    }

    /// Why an order through a key kept here but not opened this run wasn't signed.
    /// ponytail: the time reads HH:MM, where Swift followed the system's short time style.
    pub fn unopened() -> String {
        let again = UNANSWERED_AT.lock().expect("agents").map(|(_, at)| at + chrono::Duration::seconds(QUIET_FOR as i64));
        let when = again.map_or(" The next order asks again,".to_string(), |t| format!(" Orders through it ask again from {},", t.format("%H:%M")));
        format!("The Hyperliquid trading key opens once with {PRESENCE} after ihatecrypto starts, and it wasn't opened, so this order wasn't signed. \
                 The stop and take on the exchange still protect the position.{when} and any move you confirm with {PRESENCE} opens it.")
    }

    /// Sealed under Windows Hello: when the wallet's seed is, and a Ledger's key always (it has no seed here).
    pub fn sealed_for(id: &str) -> bool { seeds::sealed(&seed_file(id)).ok().flatten().is_none_or(|s| s.presence) }

    fn read_sealed(id: &str) -> Option<Sealed> { serde_json::from_slice(&unprotect(&read(&file(id))?).ok()?).ok() }

    fn open_with(s: &Sealed, master: &Key, name: &str) -> Option<Zeroizing<Vec<u8>>> { open(&subkey(master, name), &s.sealed, s.address.as_bytes()) }

    /// Whether there is a trading key for `id`, without opening it.
    pub fn has(id: &str) -> bool { exists(&file(id)) || (!sealed_for(id) && exists(&plain(id))) }

    /// Its address, without opening it. The file's address is taken only as the trusted state recorded it when the key
    /// was sealed: a file with another address put there would get that address approved.
    pub fn address(id: &str) -> Option<String> {
        if let Some(s) = read_sealed(id) {
            return (trusted::state().prefs.get(&record(id)) == Some(&Pref::String(s.address.clone()))).then_some(s.address);
        }
        if sealed_for(id) { return None }
        load_with(id, false).and_then(|k| wallet::keys::evm_address(&k).ok())
    }

    /// The key: from memory, or opened with Windows Hello's key — an open one, or one prompt (closing by itself after a
    /// minute, so an order no one is there for doesn't hold the signer).
    pub fn load(id: &str) -> Option<Zeroizing<Vec<u8>>> { load_with(id, true) }

    fn load_with(id: &str, prompt: bool) -> Option<Zeroizing<Vec<u8>>> {
        let n = name(id);
        if let Some(k) = OPENED.lock().expect("agents").get(&n) { return Some(k.clone()) }
        if let Some(s) = read_sealed(id) {
            if !prompt || !asks_now() { return None }
            let master = current_key().map(Ok).unwrap_or_else(|| super::key(
                "open your Hyperliquid trading key until ihatecrypto restarts: orders then go without Windows Hello; it can't withdraw money", false, Some(60)));
            let Some(key) = master.ok().and_then(|m| open_with(&s, &m, &n)) else { return unanswered() };
            OPENED.lock().expect("agents").insert(n, key.clone());
            return Some(key);
        }
        // A DPAPI-only key beside a wallet whose keys are sealed: never ours to take.
        if sealed_for(id) || trusted::state().prefs.contains_key(&record(id)) { return None }
        unprotect(&read(&plain(id))?).ok()
    }

    /// Every sealed trading key, opened with the key Windows Hello just gave: the first order after that asks nothing.
    pub fn open_all(master: &Key) {
        for f in files().into_iter().filter(|f| f.starts_with("agent-") && f.ends_with(".sealed")) {
            let n = f.trim_end_matches(".sealed").to_string();
            if OPENED.lock().expect("agents").contains_key(&n) { continue }
            let Some(s) = read(&f).and_then(|b| unprotect(&b).ok()).and_then(|j| serde_json::from_slice::<Sealed>(&j).ok()) else { continue };
            if let Some(key) = open_with(&s, master, &n) { OPENED.lock().expect("agents").insert(n, key); }
        }
    }

    /// 32 random bytes that make a valid secp256k1 key; retried in the vanishing case they don't.
    pub fn make() -> Result<Zeroizing<Vec<u8>>> {
        for _ in 0..4 {
            let bytes = random::<32>();
            if wallet::keys::evm_address(&*bytes).is_ok() { return Ok(Zeroizing::new(bytes.to_vec())) }
        }
        fail("Couldn't create the agent key.")
    }

    pub fn save(key: &[u8], id: &str, within: Option<u64>) -> Result<()> {
        let n = name(id);
        if !sealed_for(id) {
            write(&plain(id), &protect(key)?)?;
            remove(&file(id));
            OPENED.lock().expect("agents").remove(&n);
            return Ok(());
        }
        let address = wallet::keys::evm_address(key)?;
        let master = match current_key() { Some(k) => k, None => super::key("keep your Hyperliquid trading key under Windows Hello", false, within)? };
        let sealed = Sealed { version: 2, address: address.clone(), sealed: seal(&subkey(&master, &n), key, address.as_bytes())? };
        let json = Zeroizing::new(serde_json::to_vec(&sealed).expect("json"));
        write(&file(id), &protect(&json)?)?;
        // With Windows Hello's key in hand: sealed into the trusted state at once.
        trusted::mutate(None, |s| { s.prefs.insert(record(id), Pref::String(address)); })?;
        remove(&plain(id));
        OPENED.lock().expect("agents").insert(n, Zeroizing::new(key.to_vec()));
        Ok(())
    }

    pub fn delete(id: &str) {
        remove(&file(id));
        remove(&plain(id));
        OPENED.lock().expect("agents").remove(&name(id));
    }

    /// For tests: forget the opened keys, as a new signer process would.
    pub fn forget() {
        OPENED.lock().expect("agents").clear();
        *UNANSWERED_AT.lock().expect("agents") = None;
    }
}

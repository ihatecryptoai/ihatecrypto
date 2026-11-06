//! The engine's own files, beside the signer's (Store.swift): %LOCALAPPDATA%\ihatecrypto\engine on Windows
//! (ihatecrypto-dev for a debug build, or IHATECRYPTO_ENGINE_DIR). Settings and the chat as plain JSON in the shapes
//! Swift's JSONEncoder wrote, so a person switching from the Swift engine keeps them; model keys and sign-ins encrypted
//! to the Windows user (DPAPI, `platform::protect`), as the Mac keeps them in the Keychain. What the person chose is
//! `Settings`, with its `mirror()` (Modules.swift) into what trade reads.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::sync::LazyLock;
use std::sync::atomic::Ordering;

use ihatecrypto_core::foundation;
use ihatecrypto_core::modules::Modules;
use ihatecrypto_core::risk::RiskPolicy;
use llm::Credential;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use trade::contract_calls::ContractCallGuard;
use zeroize::Zeroizing;

use crate::from_core::ExamResult;
use crate::platform;

// MARK: The folder

#[cfg(not(test))]
static FOLDER: LazyLock<PathBuf> = LazyLock::new(|| {
    if cfg!(debug_assertions) && let Some(dir) = std::env::var_os("IHATECRYPTO_ENGINE_DIR") {
        return made(PathBuf::from(dir));
    }
    let product = if cfg!(debug_assertions) { "ihatecrypto-dev" } else { "ihatecrypto" };
    // Off Windows only the development build runs (the window on a Mac, with the stand-in signer).
    let base = if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir) } else { std::env::temp_dir() };
    made(base.join(product).join("engine"))
});

#[cfg(not(test))]
fn made(dir: PathBuf) -> PathBuf {
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(not(test))]
pub fn folder() -> PathBuf { FOLDER.clone() }

#[cfg(test)]
thread_local! {
    static TEST_FOLDER: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// A test's own folder, never the real one: a test that didn't set one fails here instead of touching it.
#[cfg(test)]
pub fn folder() -> PathBuf {
    TEST_FOLDER.with(|f| f.borrow().clone()).expect("a test keeps its files in store::test_folder()")
}

/// A fresh, empty folder for this test's thread, removed when the guard drops.
#[cfg(test)]
pub fn test_folder() -> TestFolder {
    let mut id = [0u8; 8];
    getrandom::fill(&mut id).expect("randomness");
    let dir = std::env::temp_dir().join(format!("ihatecrypto-engine-test-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    std::fs::create_dir_all(&dir).expect("a test folder");
    TEST_FOLDER.with(|f| *f.borrow_mut() = Some(dir.clone()));
    TestFolder(dir)
}

#[cfg(test)]
pub struct TestFolder(pub PathBuf);

#[cfg(test)]
impl Drop for TestFolder {
    fn drop(&mut self) {
        TEST_FOLDER.with(|f| *f.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn url(name: &str) -> PathBuf { folder().join(name) }

// MARK: Files

/// What was saved under `name`, read as Swift's JSONDecoder reads it; None when missing or unreadable.
pub fn load<T: DeserializeOwned>(name: &str) -> Option<T> {
    std::fs::read(url(name)).ok().and_then(|data| serde_json::from_slice(&data).ok())
}

/// `value` saved under `name`, whole or not at all (Swift's `.atomic`).
pub fn save<T: Serialize + ?Sized>(value: &T, name: &str) {
    if let Ok(data) = serde_json::to_vec(value) { let _ = write(name, &data); }
}

/// Bytes saved under `name` atomically: written beside it, then moved over it.
pub fn write(name: &str, data: &[u8]) -> std::io::Result<()> { write_at(&url(name), data) }

pub fn write_at(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut id = [0u8; 6];
    let _ = getrandom::fill(&mut id);
    let tmp = path.with_extension(format!("tmp-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })
}

/// A per-wallet file: wallet «1» keeps the name it had before there were several (ChatCommands.swift's
/// `Engine.scoped`, the Mac's Workspace.scoped): «costs.json» → «costs-3.json».
pub fn scoped(file: &str, wallet: Option<&str>) -> String {
    match wallet {
        Some(w) if w != signer_api::SignerWallet::FIRST_ID => match file.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}-{w}.{ext}"),
            None => format!("{file}-{w}."),
        },
        _ => file.into(),
    }
}

// MARK: Model sign-ins

/// Every provider's credential, one DPAPI blob.
pub fn credentials() -> HashMap<String, Credential> {
    let Some(plain) = std::fs::read(url("credentials.bin")).ok().and_then(|blob| unprotect(&blob)) else { return HashMap::new() };
    serde_json::from_slice(&plain).unwrap_or_default()
}

pub fn set_credential(credential: Option<Credential>, provider: &str) {
    let mut all = credentials();
    match credential { Some(c) => { all.insert(provider.into(), c); } None => { all.remove(provider); } }
    let Ok(plain) = serde_json::to_vec(&all).map(Zeroizing::new) else { return };
    if let Some(blob) = protect(&plain) { let _ = write("credentials.bin", &blob); }
}

pub fn protect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { platform::protect(data) }
pub fn unprotect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { platform::unprotect(data) }

// MARK: Settings

/// What the person chose, kept between launches (settings.json).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// «en», «ru» or «es»; None: the system's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The window's own texts: «en» or «ru»; None: Windows' language (Engine's app_russian).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The model chosen for each account (Models.model(of:)); `model` is a build before this one's choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<HashMap<String, String>>,
    /// The second check (on unless turned off, as on the Mac): the model reads every command again, and a trade plan
    /// against the user's words, before a card is drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_check: Option<bool>,
    /// The second check may also ask TypeSafe Jev through OpenRouter while the chat runs elsewhere: only when turned on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jev_open_router: Option<bool>,
    /// The canvas open last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<String>,
    /// «Addresses for balances», as on the Mac: read-only addresses whose balances show while no wallet is here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_solana: Option<String>,
    #[serde(rename = "watchEVM", default, skip_serializing_if = "Option::is_none")]
    pub watch_evm: Option<String>,
    /// The person's say on single tokens, by Holding.id: true shows one, false hides it; none follows looksReal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_choices: Option<HashMap<String, bool>>,
    /// Settings › Modules: «protocol.kamino=0,network.base=1» (Modules.stored), and the modules met so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules_known: Option<Vec<String>>,
    /// Settings › Trading agent: the risk rules (None: Balanced) and Safe search (None: on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RiskPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_search: Option<bool>,
    /// Settings › Wallet: EIP-7702 batches (None: off) and the cap for unknown protocols in dollars (None: $50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batches: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_cap: Option<f64>,
    /// The addresses whose Hyperliquid trading key the chat already offered (TradingKey.offerTradingKey).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trading_key_offered: Option<Vec<String>>,
    /// «Pause schedule»: nothing scheduled runs until it's resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_paused: Option<bool>,
    /// The app's lock (lock.rs): a code is set (the signer's last word, so a launch is locked before it answers), five
    /// wrong codes turned it off (only Windows Hello opens the app), and the minutes away before it locks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_set: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_off: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_after: Option<i64>,
    /// Settings › General › Notifications: the kinds turned off (Notice raw values); none: every kind on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices_off: Option<Vec<String>>,
    /// Settings › Background › Open at login, as Windows has it (main.rs sets the registry's Run key and says back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_at_login: Option<bool>,
    /// Settings › AI model: every request reasons first (the Mac's «Think before answering»).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    /// The models' Check results, by «provider/model» (ModelExam).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_scores: Option<HashMap<String, ExamResult>>,
    /// Settings › Background › Keep always running, as Windows has it (main.rs keeps a scheduled task and says back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_running: Option<bool>,
}

impl Settings {
    /// Settings › Modules, «one transaction for approve and swap» and the contract cap, handed to what trade reads
    /// (Modules.swift's `mirror`; the Mac's UserDefaults). Kept in memory only, so another engine of the same program
    /// can't change what this one refuses.
    pub fn mirror(&self) {
        let known = self.modules_known.as_ref().map(|k| k.iter().cloned().collect::<HashSet<_>>());
        trade::set_modules(Modules::from_stored(self.modules.as_deref().unwrap_or(""), known));
        trade::BATCHES_ON.store(self.batches == Some(true), Ordering::Relaxed);
        ContractCallGuard::set_cap_usd(self.contract_cap.and_then(|c| foundation::decimal(&c.to_string())));
    }
}

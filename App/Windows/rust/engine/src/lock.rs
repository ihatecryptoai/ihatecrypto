//! The app's lock (Lock.swift), as the Mac's AppLock (Passcode.swift): with a passcode set, the window shows only the
//! lock screen at launch, after `lockAfter` minutes away from the PC, when it sleeps or its session locks, on «lock» and
//! Ctrl+L. The signer checks the code (`EnterCode`) and turns it off after five wrong ones; Windows Hello opens the lock
//! too. Only the window is hidden: the schedule, stops and alerts go on. While locked the window gets none of the
//! balances, the chat or the history: the engine sends it the lock alone.
//! Layer 2 adds here: `make_code` and `turn_code_off`, which go through wallet_ops.rs's `signer_call`.

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;
use signer_api::{SignerRequest, passcode};
use wallet::WalletError;

use crate::engine::Engine;
use crate::platform::{self, Hello};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockView {
    /// A passcode is set: the signer's word, kept in the settings so a launch is locked before the signer answers.
    pub code_set: bool,
    pub locked: bool,
    /// Five wrong codes turned the code off: only Windows Hello opens the app — after a restart too.
    pub code_off: bool,
    /// Why the last code or Windows Hello didn't open it («Wrong code · 3 tries left»).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Minutes away before it locks: 1, 5, 15 or 60.
    pub lock_after: i64,
    pub checking: bool,
    /// Windows Hello stood in for a forgotten code: Settings › Wallet opens on making a new one.
    pub offer_new_code: bool,
    /// How many times it was offered: each «Forgot the code?» opens Settings › Wallet again, as each Mac password
    /// unlock does.
    pub new_code_offers: i64,
    /// Scheduled tasks keep running while it's locked: the lock screen says so.
    pub tasks_run: bool,
}

impl Default for LockView {
    fn default() -> Self {
        LockView { code_set: false, locked: false, code_off: false, problem: None, lock_after: 5, checking: false, offer_new_code: false,
                   new_code_offers: 0, tasks_run: false }
    }
}

/// What the window gets while it's locked: the lock and the language, nothing else.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LockedSnapshot {
    pub lock: LockView,
    pub russian: bool,
    pub language: String,
}

pub const LOCK_CHOICES: [i64; 4] = [1, 5, 15, 60];
/// Requests the window may make while locked: the lock's own, and what starts it.
pub const WHILE_LOCKED: [&str; 4] = ["hello", "unlock", "unlockHello", "signerPid"];

/// The engine's own Windows Hello key, apart from the signer's: one per copy of the app.
pub const UNLOCK_KEY: &str = if cfg!(debug_assertions) { "ihatecrypto-dev-unlock" } else { "ihatecrypto-unlock" };

/// The signer's words for a code, in the window's language.
pub fn localized_code_error(text: &str, russian: bool) -> String {
    if !russian { return text.into() }
    if let Some(rest) = text.strip_prefix("Wrong code · ")
        && let Some((left, tail)) = rest.split_once(' ')
        && !left.is_empty() && left.bytes().all(|b| b.is_ascii_digit()) && (tail == "try left" || tail == "tries left") {
        return format!("Неверный код · осталось попыток: {left}");
    }
    match text {
        "The code is 6 digits." => "Код — 6 цифр.",
        "All the same digit is the first code anyone tries." => "Одну и ту же цифру пробуют первой.",
        "A straight run like 123456 is guessed first." => "Ряд вроде 123456 угадывают первым.",
        "The signer couldn't count this try, so it didn't check the code: try again." => "Подписант не смог засчитать попытку и не проверил код: попробуй ещё раз.",
        _ => text,
    }.into()
}

/// What locks the app on the watch's tick: a gap in the timer (the PC slept), time away, or a locked session — while a
/// code is set and it isn't locked already.
pub fn locks_now(code_set: bool, locked: bool, slept: bool, idle_ms: u32, lock_after_minutes: i64, session_locked: bool) -> bool {
    code_set && !locked && (slept || i64::from(idle_ms) >= lock_after_minutes * 60_000 || session_locked)
}

impl Engine {
    pub fn lock_view(&self) -> LockView {
        let st = self.st();
        let mut v = st.lock.clone();
        v.code_set = st.settings.code_set == Some(true);
        v.code_off = st.settings.code_off == Some(true);
        let after = st.settings.lock_after.unwrap_or(5);
        v.lock_after = if LOCK_CHOICES.contains(&after) { after } else { 5 };
        // What keeps running while locked, as the Mac's scheduleEntries: tasks, recurring buys and the guarded positions.
        v.tasks_run = !st.tasks.is_empty() || !st.positions.is_empty();
        v
    }

    pub fn locked_snapshot(&self) -> LockedSnapshot {
        let lock = self.lock_view();
        let st = self.st();
        LockedSnapshot { lock, russian: st.russian(), language: st.settings.language.clone().unwrap_or_else(|| "auto".into()) }
    }

    /// Before the signer has answered: locked if the last run had a code, or had it turned off by five wrong ones.
    pub fn lock_at_launch(&self) {
        {
            let mut st = self.st_mut();
            st.lock.locked = st.settings.code_set == Some(true) || st.settings.code_off == Some(true);
        }
        self.start_lock_watch();
    }

    /// The signer's `codeSet` after each status read. Its first answer after launch locks a window the settings left open.
    pub fn lock_heard(&self, code_set: bool) {
        let (heard, locked) = { let st = self.st(); (st.lock_heard_signer, st.lock.locked) };
        self.set_settings(|s| {
            s.code_set = Some(code_set);
            if code_set { s.code_off = None }
            // No code any more while locked (five wrong codes, or the signer couldn't read it): only Windows Hello opens it.
            else if locked { s.code_off = Some(true) }
        });
        let mut st = self.st_mut();
        if code_set && !heard { st.lock.locked = true }
        st.lock_heard_signer = true;
    }

    /// Hides the window, ends the signing session; the window closes the browser (the `locked` event).
    pub async fn lock_app(&self) {
        if self.st().settings.code_set != Some(true) { return }
        {
            let mut st = self.st_mut();
            st.lock.locked = true;
            st.lock.problem = None;
        }
        // The window closes the browser: a site's request waiting is answered no (the Mac's sheet goes with its page).
        self.decline_site(&self.say("Locked before an answer: nothing was signed.", "Заблокировано до ответа: ничего не подписано."));
        self.out.event("locked", &HashMap::<String, String>::new());
        let _ = self.signer.send(&SignerRequest::EndSession {}).await;
        self.changed();
    }

    /// The code on the lock screen: the signer checks it and counts the wrong ones; it opens no signing session.
    pub async fn unlock(&self, code: &str) {
        {
            let mut st = self.st_mut();
            if !st.lock.locked || st.lock.checking { return }
            st.lock.checking = true;
        }
        self.changed();
        let reply = self.signer.send(&SignerRequest::EnterCode { code: code.into(), sign: false }).await;
        match reply {
            Err(_) => {
                let problem = self.say("The signer didn't answer: try again, or use Windows Hello.", "Подписант не ответил: попробуй ещё раз или открой через Windows Hello.");
                self.st_mut().lock.problem = Some(problem);
            }
            Ok(r) if r.ok => self.open_lock(),
            Ok(r) if r.code_off == Some(true) => {
                self.set_settings(|s| { s.code_off = Some(true); s.code_set = Some(false) });
                self.st_mut().lock.problem = None;
            }
            Ok(r) => {
                let russian = self.st().russian();
                self.st_mut().lock.problem = r.error.map(|e| localized_code_error(&e, russian));
            }
        }
        self.st_mut().lock.checking = false;
        self.changed();
    }

    /// Windows Hello in its own window: ihatecrypto never sees the PIN, the face or the fingerprint. `forgot`: «Forgot
    /// the code?» — opened by Windows Hello, then Settings › Wallet offers a new code (the Mac's unlockWithMacPassword).
    /// Windows Hello stands for both of the Mac's ways in: its touch and its password.
    pub async fn unlock_with_hello(&self, forgot: bool) {
        if !self.st().lock.locked { return }
        let reason = self.say("open ihatecrypto", "открыть ihatecrypto");
        let mut challenge = [0u8; 32];
        let _ = getrandom::fill(&mut challenge);
        // The window lets this process, and Hello's dialog, come forward, as it does for the signer's.
        self.out.event("signerForeground", &HashMap::from([("pid", std::process::id().to_string())]));
        // A key of the engine's own: a signature from it means Windows Hello saw the person; nothing is kept from it.
        // Its dialog blocks until answered: off the engine's thread.
        let answer = tokio::task::spawn_blocking(move || platform::hello(UNLOCK_KEY, &challenge, &reason)).await.unwrap_or(Hello::Failed);
        match answer {
            Hello::Signed => {}
            Hello::Cancelled => { self.changed(); return }
            Hello::Failed => {
                let problem = self.say("Windows Hello didn't open it: set up a PIN in Windows Settings › Accounts › Sign-in options.",
                                       "Windows Hello не открыл: задай PIN в «Параметры Windows › Учетные записи › Варианты входа».");
                self.st_mut().lock.problem = Some(problem);
                self.changed();
                return;
            }
        }
        let was_off = self.st().settings.code_off == Some(true);
        self.open_lock();
        if was_off || forgot {
            self.set_settings(|s| s.code_off = None);
            let mut st = self.st_mut();
            st.lock.offer_new_code = true;
            st.lock.new_code_offers += 1;
        }
        self.changed();
    }

    fn open_lock(&self) {
        let mut st = self.st_mut();
        st.lock.locked = false;
        st.lock.problem = None;
    }

    /// Makes or changes the code; the signer asks Windows Hello first (signer_call reads its status again after).
    pub async fn make_code(&self, code: &str) -> Result<(), WalletError> {
        if let Some(problem) = passcode::problem(code) { return Err(WalletError(self.localized_code_error(&problem))) }
        if let Err(e) = self.signer_call(SignerRequest::SetCode { code: code.into() }).await { return Err(WalletError(self.localized_code_error(&e.0))) }
        self.st_mut().lock.offer_new_code = false;
        self.changed();
        Ok(())
    }

    pub async fn turn_code_off(&self) -> Result<(), WalletError> {
        self.signer_call(SignerRequest::ClearCode {}).await?;
        self.set_settings(|s| s.code_off = None);
        Ok(())
    }

    pub fn set_lock_after(&self, minutes: i64) {
        if !LOCK_CHOICES.contains(&minutes) { return }
        self.set_settings(|s| s.lock_after = Some(minutes));
    }

    /// The signer's words for a code, in the window's language.
    pub fn localized_code_error(&self, text: &str) -> String { localized_code_error(text, self.st().russian()) }

    /// Every 15 s: time away from the PC, its session locked, or a gap in the timer (the PC slept) locks the app.
    fn start_lock_watch(&self) {
        let me = self.rc();
        tokio::task::spawn_local(async move {
            let mut last = chrono::Utc::now();
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let now = chrono::Utc::now();
                let slept = (now - last).num_milliseconds() > 75_000;
                last = now;
                let screen_locked = platform::session_locked();
                let end = (slept || screen_locked) && !me.st().session_ended_for_lock;
                if end {
                    me.st_mut().session_ended_for_lock = true;
                    let _ = me.signer.send(&SignerRequest::EndSession {}).await;
                }
                if !slept && !screen_locked { me.st_mut().session_ended_for_lock = false }
                let (code_set, locked) = { let st = me.st(); (st.settings.code_set == Some(true), st.lock.locked) };
                if locks_now(code_set, locked, slept, platform::idle_ms(), me.lock_view().lock_after, platform::session_locked()) {
                    me.lock_app().await;
                }
            }
        });
    }
}

//! The link against the Rust signer itself (when it's built beside this test) and against stand-ins that die.

use super::*;

/// The signer's folder is the process's environment, which every signer started inherits: one test at a time holds it,
/// each with a fresh folder of its own.
#[cfg(not(windows))]
static SIGNER_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(not(windows))]
pub(crate) fn fresh_signer_dir(name: &str) -> std::sync::MutexGuard<'static, ()> {
    let held = SIGNER_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("ihc-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: changed only while the lock is held, before the signer starts; the child inherits it.
    unsafe { std::env::set_var("IHATECRYPTO_SIGNER_DIR", &dir) }
    held
}

/// The signer cargo built next to this test's binary (target/<profile>/), if it did.
pub(crate) fn built_signer() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.parent()?.to_path_buf();
    let exe = dir.join(if cfg!(windows) { "ihatecrypto-signer.exe" } else { "ihatecrypto-signer" });
    exe.exists().then_some(exe)
}

pub(crate) fn runtime() -> tokio::runtime::Runtime { tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap() }

/// Off Windows the stand-in parent check lets a test process talk to the signer; on Windows only ihatecrypto-engine.exe may.
#[cfg(not(windows))]
#[test]
fn the_real_signer_answers_one_request_at_a_time() {
    let Some(exe) = built_signer() else { eprintln!("ihatecrypto-signer isn't built beside the tests: `cargo build -p signer` first"); return };
    let _held = fresh_signer_dir("link");
    let link = Arc::new(SignerLink::new(exe));
    runtime().block_on(async {
        let reply = link.send(&SignerRequest::Status {}).await.unwrap();
        assert!(reply.ok && reply.has_wallet == Some(false));
        let pid = link.pid().unwrap();
        // Side by side, the two wait their turn on the same pipes and both come back.
        let (a, b) = tokio::join!(link.send(&SignerRequest::Status {}), link.send(&SignerRequest::EndSession {}));
        assert!(a.unwrap().ok && b.unwrap().ok);
        assert_eq!(link.pid(), Some(pid)); // the same process served them
    });
}

#[cfg(unix)]
#[test]
fn a_signer_that_dies_is_started_once_more_then_named_in_the_report() {
    use std::os::unix::fs::PermissionsExt;
    let script = std::env::temp_dir().join(format!("ihc-dying-signer-{}", std::process::id()));
    std::fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = SignerLink::new(script.clone());
    let reports = Arc::new(Blocking::new(Vec::new()));
    let r = reports.clone();
    link.on_stopped(Arc::new(move |s| r.lock().unwrap().push(s)));
    let error = runtime().block_on(link.send(&SignerRequest::SignPersonal { message: b"0x secret amounts".to_vec() })).unwrap_err();
    assert_eq!(error.0, "The signer stopped answering: nothing was signed.");
    let reports = reports.lock().unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0], "the signer stopped answering on «signPersonal»; it was started again");
    assert!(!reports.iter().any(|r| r.contains("secret")));
    assert_eq!(link.pid(), None);
    let _ = std::fs::remove_file(script);
}

#[test]
fn a_missing_signer_says_where_it_looked() {
    let link = SignerLink::new(PathBuf::from("/nonexistent/ihatecrypto-signer"));
    assert!(!link.is_available());
    let e = runtime().block_on(link.send(&SignerRequest::Status {})).unwrap_err();
    assert_eq!(e.0, "The signer didn't start: /nonexistent/ihatecrypto-signer is missing or Windows refused it.");
}

/// On Windows the signer serves only ihatecrypto's engine: a test process that starts it gets no answer, and the link
/// says so without signing anything.
#[cfg(windows)]
#[test]
fn the_real_signer_refuses_a_parent_that_isnt_the_engine() {
    let Some(exe) = built_signer() else { eprintln!("ihatecrypto-signer isn't built beside the tests: `cargo build -p signer` first"); return };
    let link = SignerLink::new(exe);
    let e = runtime().block_on(link.send(&SignerRequest::Status {})).unwrap_err();
    assert_eq!(e.0, "The signer stopped answering: nothing was signed.");
}

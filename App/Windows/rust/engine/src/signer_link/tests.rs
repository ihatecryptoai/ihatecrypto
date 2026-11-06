//! The link against the Rust signer itself (when it's built beside this test) and against stand-ins that die.

use super::*;

/// The signer cargo built next to this test's binary (target/<profile>/), if it did.
fn built_signer() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.parent()?.to_path_buf();
    let exe = dir.join(if cfg!(windows) { "ihatecrypto-signer.exe" } else { "ihatecrypto-signer" });
    exe.exists().then_some(exe)
}

fn runtime() -> tokio::runtime::Runtime { tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap() }

#[test]
fn the_real_signer_answers_one_request_at_a_time() {
    let Some(exe) = built_signer() else { eprintln!("ihatecrypto-signer isn't built beside the tests: `cargo build -p signer` first"); return };
    let dir = std::env::temp_dir().join(format!("ihc-engine-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: set before the signer starts; the child inherits it. The Debug signer keeps its files there.
    unsafe { std::env::set_var("IHATECRYPTO_SIGNER_DIR", &dir) }
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
    let _ = std::fs::remove_dir_all(dir);
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

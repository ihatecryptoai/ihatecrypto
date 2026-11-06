//! On a real Windows only: the signer's calls into winsec, linked as a library.
#![cfg(windows)]

use signer::platform;

#[test]
fn dpapi_seals_to_this_user_and_opens_again() {
    let secret = b"a seed that must not be on disk in the clear";
    let sealed = platform::protect(secret).expect("DPAPI encrypts");
    assert!(!sealed.windows(secret.len()).any(|w| w == secret));
    assert_eq!(&*platform::unprotect(&sealed).expect("DPAPI opens it"), secret);
    let mut broken = sealed.to_vec();
    let last = broken.len() - 1;
    broken[last] ^= 1;
    assert!(platform::unprotect(&broken).is_none());
    assert!(!platform::is_stub());
}

#[test]
fn windows_hello_is_asked_about_not_faked() {
    // Over SSH or on a runner nobody can answer a prompt: whatever Windows says, the call returns and the fake never runs.
    let _ = platform::hello_available();
    unsafe { std::env::set_var("WINSEC_FAKE_HELLO", "1") }
    assert!(!platform::hello_in_tpm("ihatecrypto-test-key-that-doesnt-exist"));
}

#[test]
fn the_signer_serves_only_its_engine() {
    // Started by the test harness, not by ihatecrypto-engine.exe: winsec's parent check refuses, exit code 3.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ihatecrypto-signer")).output().unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("started by something other than ihatecrypto's engine"));
}

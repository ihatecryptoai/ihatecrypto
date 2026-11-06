//! The signer's door on Windows (`Host.swift`): a request's JSON in, its reply's JSON out — the same `SignerRequest`
//! and `SignerReply` as the Mac's XPC — one at a time, over the pipes of the engine that started it.

use std::sync::Mutex;

use signer_api::{SignerReply, SignerRequest};

use crate::{core, platform, vault};

/// What hardening couldn't do, said in every status reply, so the window can show it.
pub static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn handle(body: &[u8]) -> Vec<u8> {
    let answer = match serde_json::from_slice::<SignerRequest>(body) {
        Ok(request) => {
            let status = matches!(request, SignerRequest::Status {});
            vault::begin_request();
            match core::handle(request) {
                Ok(mut r) => {
                    if status {
                        r.checks.extend(NOTES.lock().expect("notes").iter().cloned());
                        r.checks.extend(presence());
                    }
                    r
                }
                Err(e) => SignerReply::error(e.0),
            }
        }
        Err(e) => SignerReply::error(format!("The signer couldn't read the request: {e}")),
    };
    serde_json::to_vec(&answer).unwrap_or_default()
}

/// Whether Windows Hello can guard a wallet here, and whether its key is in the TPM.
fn presence() -> Vec<String> {
    if !platform::hello_available() { return vec!["Windows Hello isn't set up: only a test wallet can be made.".into()] }
    if vault::master().is_none() { return vec!["Windows Hello is ready.".into()] }
    vec![if platform::hello_in_tpm(&vault::hello_key()) { "Windows Hello is ready. Its key is in the TPM." }
         else { "Windows Hello is ready, but Windows can't show its key is in a TPM: it may be kept in software." }.into()]
}

/// The hardening the process got, in words.
pub fn describe(failed: i32) -> Vec<String> {
    let mut notes = Vec::new();
    if failed & 1 != 0 { notes.push("Other programs may be able to read the signer's memory: its process rule wasn't set.".into()) }
    if failed & (2 | 4) != 0 { notes.push("Other programs may be able to steer the signer's threads: their rule wasn't set.".into()) }
    if failed & 8 != 0 { notes.push("Some of Windows' protections for the signer (no extension DLLs, no child processes) weren't turned on.".into()) }
    if !platform::folder_protected() && !platform::is_stub() {
        notes.push("ihatecrypto is installed in a folder your Windows user can write to: install it for all users so no program can replace it.".into());
    }
    notes
}

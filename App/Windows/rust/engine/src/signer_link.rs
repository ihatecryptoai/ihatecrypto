//! The engine's side of the signing process (`SignerLink.swift`): it starts `ihatecrypto-signer` from its own folder with
//! pipes, hardened from birth, and sends it one request at a time — the same SignerRequest and SignerReply as the Mac's
//! XPC, each a 4-byte little-endian length and the JSON. A signer that died is started again on the next request.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as Blocking};

use signer_api::{SignerReply, SignerRequest, read_frame, write_frame};
use tokio::sync::Mutex;
use wallet::{Result, WalletError};

use crate::platform::{self, Child};

/// Told when the signer stopped answering (crashed, or killed by another program): the window keeps it with its crash
/// reports, as the Mac collects the signer's. Only the request's name goes in, never its amounts, addresses or bytes.
pub type Stopped = Arc<dyn Fn(String) + Send + Sync>;

pub struct SignerLink {
    executable: PathBuf,
    /// One exchange on the pipes at a time: a second request — a lock's endSession, a status read — waits while a reply
    /// is awaited (a Windows Hello prompt can take a minute) instead of writing to the same pipes.
    child: Mutex<Option<Child>>,
    pid: Blocking<Option<u32>>,
    stopped: Blocking<Option<Stopped>>,
}

impl SignerLink {
    /// The signer beside this engine's own executable.
    pub fn beside_engine() -> Self {
        let me = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default();
        Self::new(me.join(if cfg!(windows) { "ihatecrypto-signer.exe" } else { "ihatecrypto-signer" }))
    }

    pub fn new(executable: PathBuf) -> Self {
        SignerLink { executable, child: Mutex::new(None), pid: Blocking::new(None), stopped: Blocking::new(None) }
    }

    pub fn is_available(&self) -> bool { self.executable.exists() }

    /// The signer's process id, for the window to let it come forward when Windows Hello asks.
    pub fn pid(&self) -> Option<u32> { *self.pid.lock().expect("pid") }

    pub fn on_stopped(&self, stopped: Stopped) { *self.stopped.lock().expect("stopped") = Some(stopped) }

    pub async fn send(&self, request: &SignerRequest) -> Result<SignerReply> {
        let body = serde_json::to_vec(request).map_err(|e| WalletError(e.to_string()))?;
        let mut held = self.child.lock().await;
        // A dead signer (killed, crashed) is started once more for this request.
        for attempt in 0..2 {
            let child = match held.take() {
                Some(c) => c,
                None => {
                    let Some(c) = platform::spawn_signer(&self.executable) else {
                        return Err(WalletError(format!("The signer didn't start: {} is missing or Windows refused it.", self.executable.display())));
                    };
                    *self.pid.lock().expect("pid") = Some(c.pid);
                    c
                }
            };
            // The pipes block: off the runtime's threads, so a Windows Hello prompt doesn't hold the engine up.
            let frame = body.clone();
            let (child, answer) = tokio::task::spawn_blocking(move || {
                let mut child = child;
                let answer = write_frame(&mut child.input, &frame).ok().and_then(|_| read_frame(&mut child.output));
                (child, answer)
            }).await.map_err(|e| WalletError(format!("The signer's exchange failed: {e}")))?;
            if let Some(answer) = answer {
                *held = Some(child);
                return serde_json::from_slice(&answer).map_err(|e| WalletError(format!("The signer's answer can't be read: {e}")));
            }
            drop(child);
            *self.pid.lock().expect("pid") = None;
            let callback = self.stopped.lock().expect("stopped").clone();
            if let Some(stopped) = callback {
                stopped(format!("the signer stopped answering on «{}»; it was started again", name(request)));
            }
            if attempt == 1 { break }
        }
        Err(WalletError("The signer stopped answering: nothing was signed.".into()))
    }
}

/// The request's case alone, as the Mac's `Mirror` label gives it.
fn name(request: &SignerRequest) -> String {
    serde_json::to_value(request).ok().and_then(|v| v.as_object()?.keys().next().cloned()).unwrap_or_else(|| "a request".into())
}

#[cfg(test)]
#[path = "signer_link/tests.rs"]
pub(crate) mod tests;

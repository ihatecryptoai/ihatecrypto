//! The Windows calls the engine makes, through winsec linked as a Rust library: the signer started hardened from birth
//! inside the engine's job object, Windows Hello for the app's own lock, how long the person has been away and whether
//! the session is locked, letting the signer's window come forward, DPAPI for the engine's own secrets. Off Windows a
//! stand-in takes winsec's place, as `winsec_stub.c` did: the signer is a plain child process, Windows Hello is faked
//! only with WINSEC_FAKE_HELLO=1, «DPAPI» is a fixed XOR and the person is never away — no build of it ships.

use std::io::{Read, Write};
use std::path::Path;

use zeroize::Zeroizing;

/// A running signer: its process id and its two pipes. Dropping it closes them, and the signer, whose input ended, stops.
pub struct Child {
    pub pid: u32,
    pub input: Box<dyn Write + Send>,
    pub output: Box<dyn Read + Send>,
    /// What keeps the process's handle (Windows) or child record (elsewhere) until the pipes are dropped.
    _process: Box<dyn Send>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hello {
    Signed,
    Cancelled,
    /// Not set up, or it failed: the lock says how to set up a PIN.
    Failed,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::fs::File;
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use winsec as w;

    pub fn spawn_signer(exe: &Path) -> Option<Child> {
        // The system's own form of the path («C:\Program Files\…»).
        let path = exe.to_string_lossy();
        let (mut pid, mut process, mut to_child, mut from_child) = (0u32, 0isize, 0isize, 0isize);
        let code = unsafe { w::winsec_spawn_signer(path.as_ptr(), path.len(), &mut pid, &mut process, &mut to_child, &mut from_child) };
        if code != 0 { return None }
        // SAFETY: winsec hands out three fresh handles that nothing else owns.
        let (input, output, process) = unsafe {
            (File::from_raw_handle(to_child as RawHandle), File::from_raw_handle(from_child as RawHandle), OwnedHandle::from_raw_handle(process as RawHandle))
        };
        Some(Child { pid, input: Box::new(input), output: Box::new(output), _process: Box::new(process) })
    }

    pub fn allow_foreground(pid: u32) -> bool { w::winsec_allow_foreground(pid) == 0 }
    pub fn idle_ms() -> u32 { w::winsec_idle_ms() }
    pub fn session_locked() -> bool { w::winsec_session_locked() == 1 }

    pub fn hello(key: &str, challenge: &[u8], reason: &str) -> Hello {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let code = unsafe {
            w::winsec_hello_sign(key.as_ptr(), key.len(), challenge.as_ptr(), challenge.len(), reason.as_ptr(), reason.len(), 1, &mut out, &mut len)
        };
        if !out.is_null() { unsafe { w::winsec_free(out, len) } }
        match code { 0 => Hello::Signed, 1 => Hello::Cancelled, _ => Hello::Failed }
    }

    /// DPAPI: encrypted to this Windows user, for the engine's model keys, sign-ins and nodes (Store.swift).
    fn transform(data: &[u8], protect: bool) -> Option<Zeroizing<Vec<u8>>> {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let code = unsafe {
            if protect { w::winsec_protect(data.as_ptr(), data.len(), &mut out, &mut len) } else { w::winsec_unprotect(data.as_ptr(), data.len(), &mut out, &mut len) }
        };
        if code != 0 || out.is_null() { return None }
        let plain = Zeroizing::new(unsafe { std::slice::from_raw_parts(out, len) }.to_vec());
        unsafe { w::winsec_free(out, len) };
        Some(plain)
    }

    pub fn protect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { transform(data, true) }
    pub fn unprotect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { transform(data, false) }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::process::{Command, Stdio};

    pub fn spawn_signer(exe: &Path) -> Option<Child> {
        let mut child = Command::new(exe).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().ok()?;
        let (input, output) = (child.stdin.take()?, child.stdout.take()?);
        Some(Child { pid: child.id(), input: Box::new(input), output: Box::new(output), _process: Box::new(child) })
    }

    pub fn allow_foreground(_pid: u32) -> bool { true }
    /// No session to watch off Windows: never away, never locked.
    pub fn idle_ms() -> u32 { 0 }
    pub fn session_locked() -> bool { false }

    pub fn hello(_key: &str, _challenge: &[u8], _reason: &str) -> Hello {
        let on = |v: &str| std::env::var(v).is_ok_and(|x| x == "1");
        if !on("WINSEC_FAKE_HELLO") { Hello::Failed } else if on("WINSEC_FAKE_CANCEL") { Hello::Cancelled } else { Hello::Signed }
    }

    /// «DPAPI» off Windows: a fixed XOR, as winsec_stub.c — it protects nothing, and no build of it ships.
    pub fn protect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { Some(Zeroizing::new(data.iter().map(|b| b ^ 0x5A).collect())) }
    pub fn unprotect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { protect(data) }
}

pub use imp::*;

//! The Windows calls the signer makes — Windows Hello, DPAPI, hardening, its pipes — through winsec, now linked as a
//! Rust library. Off Windows a stand-in takes its place, as `winsec_stub.c` did: Windows Hello is faked only with
//! WINSEC_FAKE_HELLO=1 (a signature made from the key name and challenge alone) and «DPAPI» is a fixed XOR, so nothing
//! here protects a real wallet, and no build of it ships.

use zeroize::Zeroizing;

/// winsec's status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Cancelled,
    NotFound,
    Unavailable,
    Locked,
    Failed,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn status(code: i32) -> Result<(), Status> {
    match code {
        0 => Ok(()),
        1 => Err(Status::Cancelled),
        2 => Err(Status::NotFound),
        3 => Err(Status::Unavailable),
        4 => Err(Status::Locked),
        _ => Err(Status::Failed),
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winsec as w;

    fn take(ptr: *mut u8, len: usize) -> Zeroizing<Vec<u8>> {
        let out = Zeroizing::new(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec());
        unsafe { w::winsec_free(ptr, len) };
        out
    }

    pub fn is_stub() -> bool { false }
    pub fn hello_available() -> bool { w::winsec_hello_available() == 0 }

    pub fn hello_sign(name: &str, challenge: &[u8], reason: &str, create: bool, within_ms: u32) -> Result<Zeroizing<Vec<u8>>, Status> {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let code = unsafe {
            w::winsec_hello_sign_within(name.as_ptr(), name.len(), challenge.as_ptr(), challenge.len(), reason.as_ptr(), reason.len(),
                                        create as i32, within_ms, &mut out, &mut len)
        };
        status(code)?;
        if out.is_null() { return Err(Status::Failed) }
        Ok(take(out, len))
    }

    pub fn hello_in_tpm(name: &str) -> bool { unsafe { w::winsec_hello_in_tpm(name.as_ptr(), name.len()) == 0 } }

    fn transform(data: &[u8], protect: bool) -> Option<Zeroizing<Vec<u8>>> {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let code = unsafe {
            if protect { w::winsec_protect(data.as_ptr(), data.len(), &mut out, &mut len) } else { w::winsec_unprotect(data.as_ptr(), data.len(), &mut out, &mut len) }
        };
        (code == 0 && !out.is_null()).then(|| take(out, len))
    }

    pub fn protect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { transform(data, true) }
    pub fn unprotect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { transform(data, false) }
    pub fn harden() -> i32 { w::winsec_harden() }
    pub fn parent_check(engine: &str) -> bool { unsafe { w::winsec_parent_check(engine.as_ptr(), engine.len()) == 0 } }
    pub fn parent_check_above(window: &str) -> bool { unsafe { w::winsec_parent_check_above(window.as_ptr(), window.len()) == 0 } }
    pub fn folder_protected() -> bool { w::winsec_folder_protected() == 0 }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    fn fake(var: &str) -> bool { std::env::var(var).is_ok_and(|v| v == "1") }

    pub fn is_stub() -> bool { true }
    pub fn hello_available() -> bool { fake("WINSEC_FAKE_HELLO") }

    /// 256 bytes, the same for the same key name and challenge: what an RSA-2048 PKCS#1 v1.5 signature gives.
    pub fn hello_sign(name: &str, challenge: &[u8], _reason: &str, _create: bool, _within_ms: u32) -> Result<Zeroizing<Vec<u8>>, Status> {
        if !fake("WINSEC_FAKE_HELLO") { return Err(Status::Unavailable) }
        if fake("WINSEC_FAKE_CANCEL") { return Err(Status::Cancelled) }
        let mut h: u64 = 1469598103934665603;
        for &b in name.as_bytes().iter().chain(challenge) { h = (h ^ b as u64).wrapping_mul(1099511628211) }
        Ok(Zeroizing::new((0..256u64).map(|i| { h = (h ^ i).wrapping_mul(1099511628211); (h >> 56) as u8 }).collect()))
    }

    pub fn hello_in_tpm(_name: &str) -> bool { false }
    pub fn protect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { Some(Zeroizing::new(data.iter().map(|b| b ^ 0x5A).collect())) }
    pub fn unprotect(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> { protect(data) }
    pub fn harden() -> i32 { 0 }
    pub fn parent_check(_engine: &str) -> bool { true }
    pub fn parent_check_above(_window: &str) -> bool { true }
    pub fn folder_protected() -> bool { false }
}

pub use imp::*;

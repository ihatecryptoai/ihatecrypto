//! The Windows calls the Swift signer and engine can't make from Swift: Windows Hello (WinRT), DPAPI, hardening the
//! signer's process against other programs of the same user, and starting the signer so it starts hardened.
//! A C ABI only (winsec.h); every function returns 0 for success. Buffers this library hands out are freed with
//! `winsec_free`, which overwrites them first.
#![cfg(windows)]
#![allow(clippy::missing_safety_doc)]

mod ble;
mod ledger;

use std::ffi::c_void;
use std::sync::Once;
use std::time::{Duration, Instant};

use windows::Security::Credentials::{
    KeyCredential, KeyCredentialAttestationStatus, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::Cryptography::*;
use windows::Win32::Security::WinTrust::*;
use windows::Win32::Security::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR, w};
use zeroize::Zeroize;

// Status codes, mirrored in winsec.h.
const OK: i32 = 0;
const CANCELLED: i32 = 1;
const NOT_FOUND: i32 = 2;
const UNAVAILABLE: i32 = 3;
const LOCKED: i32 = 4;
const WRONG_PARENT: i32 = 5;
const FAILED: i32 = -1;

unsafe fn text(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() { return String::new() }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

unsafe fn give(bytes: Vec<u8>, out: *mut *mut u8, out_len: *mut usize) {
    let boxed = bytes.into_boxed_slice();
    unsafe {
        *out_len = boxed.len();
        *out = Box::into_raw(boxed) as *mut u8;
    }
}

/// Frees a buffer this library returned, overwriting it first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() { return }
    let mut boxed = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) };
    boxed.zeroize();
}

/// Overwrites `len` bytes at `ptr` in a way the compiler keeps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_wipe(ptr: *mut u8, len: usize) {
    if ptr.is_null() { return }
    unsafe { std::slice::from_raw_parts_mut(ptr, len) }.zeroize();
}

#[unsafe(no_mangle)]
pub extern "C" fn winsec_is_stub() -> i32 { 0 }

// MARK: Windows Hello

/// 0: Windows Hello is set up (a PIN at least); UNAVAILABLE: it isn't, or this Windows has none.
#[unsafe(no_mangle)]
pub extern "C" fn winsec_hello_available() -> i32 {
    match KeyCredentialManager::IsSupportedAsync().and_then(|op| op.join()) {
        Ok(true) => OK,
        _ => UNAVAILABLE,
    }
}

fn status_code(status: KeyCredentialStatus) -> i32 {
    match status {
        KeyCredentialStatus::Success => OK,
        KeyCredentialStatus::UserCanceled | KeyCredentialStatus::UserPrefersPassword => CANCELLED,
        KeyCredentialStatus::NotFound => NOT_FOUND,
        KeyCredentialStatus::SecurityDeviceLocked => LOCKED,
        _ => FAILED,
    }
}

fn credential(name: &HSTRING, create: bool) -> Result<KeyCredential, i32> {
    let opened = KeyCredentialManager::OpenAsync(name).and_then(|op| op.join()).map_err(|_| FAILED)?;
    let status = opened.Status().map_err(|_| FAILED)?;
    if status == KeyCredentialStatus::Success { return opened.Credential().map_err(|_| FAILED) }
    if status != KeyCredentialStatus::NotFound || !create { return Err(status_code(status)) }
    // A new key in Windows Hello's container, in the TPM where there is one. FailIfExists: never over a key that
    // seals a wallet already.
    let made = KeyCredentialManager::RequestCreateAsync(name, KeyCredentialCreationOption::FailIfExists)
        .and_then(|op| op.join())
        .map_err(|_| FAILED)?;
    match made.Status().map_err(|_| FAILED)? {
        KeyCredentialStatus::Success => made.Credential().map_err(|_| FAILED),
        other => Err(status_code(other)),
    }
}

/// Signs `challenge` with the Windows Hello key `name` (made first when `create` and there is none): Windows asks for
/// the face, the fingerprint or the PIN. While it asks, the signer's own window shows `reason`. The signature (RSA
/// PKCS#1 v1.5, the same for the same challenge) goes to `out`; the caller derives its key from it and frees it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_hello_sign(
    name: *const u8, name_len: usize, challenge: *const u8, challenge_len: usize, reason: *const u8, reason_len: usize,
    create: i32, out: *mut *mut u8, out_len: *mut usize,
) -> i32 {
    let name = HSTRING::from(unsafe { text(name, name_len) });
    let challenge = unsafe { std::slice::from_raw_parts(challenge, challenge_len) }.to_vec();
    let reason = unsafe { text(reason, reason_len) };
    let result = with_details(&reason, move || -> Result<Vec<u8>, i32> {
        let key = credential(&name, create != 0)?;
        let data = CryptographicBuffer::CreateFromByteArray(&challenge).map_err(|_| FAILED)?;
        let signed = key.RequestSignAsync(&data).and_then(|op| op.join()).map_err(|_| FAILED)?;
        let status = signed.Status().map_err(|_| FAILED)?;
        if status != KeyCredentialStatus::Success { return Err(status_code(status)) }
        let buffer = signed.Result().map_err(|_| FAILED)?;
        let mut array = windows::core::Array::<u8>::new();
        CryptographicBuffer::CopyToByteArray(&buffer, &mut array).map_err(|_| FAILED)?;
        let bytes = array.to_vec();
        array.as_mut().zeroize();
        Ok(bytes)
    });
    match result {
        Ok(bytes) => {
            unsafe { give(bytes, out, out_len) };
            OK
        }
        Err(code) => code,
    }
}

/// OK when Windows can attest the key `name` is in the TPM; UNAVAILABLE when it is a software key (no TPM, or
/// Windows Hello kept it outside); NOT_FOUND when there is no such key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_hello_in_tpm(name: *const u8, name_len: usize) -> i32 {
    let name = HSTRING::from(unsafe { text(name, name_len) });
    let key = match credential(&name, false) {
        Ok(key) => key,
        Err(code) => return code,
    };
    match key.GetAttestationAsync().and_then(|op| op.join()).and_then(|a| a.Status()) {
        Ok(KeyCredentialAttestationStatus::Success) => OK,
        Ok(KeyCredentialAttestationStatus::NotSupported) => UNAVAILABLE,
        _ => FAILED,
    }
}

// MARK: The signer's window while Windows Hello asks

const DETAILS_CLASS: PCWSTR = w!("ihatecryptoSignerDetails");
static REGISTER: Once = Once::new();

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

unsafe extern "system" fn details_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                FillRect(dc, &rect, HBRUSH(GetStockObject(WHITE_BRUSH).0));
                let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                let font = CreateFontW(-(15 * dpi / 72), 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                       CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
                let old = SelectObject(dc, font.into());
                // The text lives in a property (a Box<Vec<u16>> with_details frees), not the caption: that stays short.
                let held = GetPropW(hwnd, w!("reason"));
                let mut buf: Vec<u16> = if held.is_invalid() { Vec::new() } else {
                    let v = &*(held.0 as *const Vec<u16>);
                    v[..v.len().saturating_sub(1)].to_vec()
                };
                let pad = 16 * dpi / 96;
                let mut inner = RECT { left: rect.left + pad, top: rect.top + pad, right: rect.right - pad, bottom: rect.bottom - pad };
                DrawTextW(dc, &mut buf, &mut inner, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);
                SelectObject(dc, old);
                let _ = DeleteObject(font.into());
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Runs `body` (the Windows Hello request) on its own thread while this thread shows `reason` in a small window on
/// top and brings Windows Hello's dialog forward, as it can open behind other windows.
fn with_details<T: Send + 'static>(reason: &str, body: impl FnOnce() -> T + Send + 'static) -> T {
    let worker = std::thread::spawn(body);
    let window = unsafe { details_window(reason) };
    let started = Instant::now();
    let mut msg = MSG::default();
    while !worker.is_finished() {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // It can open behind the app's window, or a moment after: brought forward while it isn't in front, for
            // half a minute at most, so a person who switched away on purpose isn't pulled back for ever.
            if started.elapsed() < Duration::from_secs(30)
                && let Ok(dialog) = FindWindowW(w!("Credential Dialog Xaml Host"), PCWSTR::null()) {
                    if GetForegroundWindow() != dialog { let _ = SetForegroundWindow(dialog); }
                    // The details stay readable beside the dialog, never over its title: above it, else below it.
                    if let Some(window) = window { keep_clear(window, dialog) }
                }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    if let Some(window) = window {
        unsafe {
            let reason = GetPropW(window, w!("reason"));
            let _ = RemovePropW(window, w!("reason"));
            let _ = DestroyWindow(window);
            if !reason.is_invalid() { drop(Box::from_raw(reason.0 as *mut Vec<u16>)) }
        }
    }
    worker.join().expect("the Windows Hello thread panicked")
}

/// Moves the details window off Windows Hello's dialog when they overlap: above it when there's room, else below it.
unsafe fn keep_clear(window: HWND, dialog: HWND) {
    unsafe {
        let (mut ours, mut theirs) = (RECT::default(), RECT::default());
        if GetWindowRect(window, &mut ours).is_err() || GetWindowRect(dialog, &mut theirs).is_err() { return }
        let overlap = ours.left < theirs.right && theirs.left < ours.right && ours.top < theirs.bottom && theirs.top < ours.bottom;
        if !overlap { return }
        let height = ours.bottom - ours.top;
        let gap = 8;
        let screen = GetSystemMetrics(SM_CYSCREEN);
        let top = if theirs.top - height - gap >= 0 { theirs.top - height - gap }
            else if theirs.bottom + gap + height <= screen { theirs.bottom + gap } else { 0 };
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), ours.left, top, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

unsafe fn details_window(reason: &str) -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        REGISTER.call_once(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(details_proc),
                hInstance: instance.into(),
                lpszClassName: DETAILS_CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&class);
        });
        // As tall as the text needs once it's wrapped at this width, in the font WM_PAINT draws it with.
        let dpi = GetDpiForSystem().max(96) as i32;
        let width = 560 * dpi / 96;
        let pad = 16 * dpi / 96;
        let dc = GetDC(None);
        let font = CreateFontW(-(15 * dpi / 72), 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                               CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
        let old = SelectObject(dc, font.into());
        let mut measure = wide(reason);
        measure.pop();
        let mut rect = RECT { left: 0, top: 0, right: width - 2 * pad, bottom: 0 };
        DrawTextW(dc, &mut measure, &mut rect, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT);
        SelectObject(dc, old);
        let _ = DeleteObject(font.into());
        ReleaseDC(None, dc);
        let mut frame = RECT { left: 0, top: 0, right: width, bottom: rect.bottom + 2 * pad };
        let _ = AdjustWindowRectEx(&mut frame, WS_POPUP | WS_CAPTION, false, WS_EX_TOPMOST | WS_EX_TOOLWINDOW);
        let height = frame.bottom - frame.top;
        let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
        let title = wide("ihatecrypto · Windows Hello");
        let window = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, DETAILS_CLASS, PCWSTR(title.as_ptr()), WS_POPUP | WS_CAPTION,
                                     x, 80 * dpi / 96, width, height, None, None, Some(instance.into()), None).ok()?;
        // Kept alive until the window is destroyed (with_details frees it).
        let text = Box::into_raw(Box::new(wide(reason)));
        let _ = SetPropW(window, w!("reason"), Some(HANDLE(text as *mut c_void)));
        let _ = ShowWindow(window, SW_SHOWNA);
        let _ = UpdateWindow(window);
        Some(window)
    }
}

// MARK: DPAPI

fn entropy() -> CRYPT_INTEGER_BLOB {
    const LABEL: &[u8] = b"ihatecrypto signer";
    CRYPT_INTEGER_BLOB { cbData: LABEL.len() as u32, pbData: LABEL.as_ptr() as *mut u8 }
}

unsafe fn taken(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    unsafe {
        let slice = std::slice::from_raw_parts_mut(blob.pbData, blob.cbData as usize);
        let bytes = slice.to_vec();
        slice.zeroize();
        let _ = LocalFree(Some(HLOCAL(blob.pbData as *mut c_void)));
        bytes
    }
}

/// Encrypts `data` to this Windows user (DPAPI, this machine's copy of the user's key).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_protect(data: *const u8, len: usize, out: *mut *mut u8, out_len: *mut usize) -> i32 {
    let input = CRYPT_INTEGER_BLOB { cbData: len as u32, pbData: data as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let e = entropy();
    if unsafe { CryptProtectData(&input, PCWSTR::null(), Some(&e), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output) }.is_err() {
        return FAILED;
    }
    unsafe { give(taken(output), out, out_len) };
    OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_unprotect(data: *const u8, len: usize, out: *mut *mut u8, out_len: *mut usize) -> i32 {
    let input = CRYPT_INTEGER_BLOB { cbData: len as u32, pbData: data as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let e = entropy();
    if unsafe { CryptUnprotectData(&input, None, Some(&e), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output) }.is_err() {
        return FAILED;
    }
    unsafe { give(taken(output), out, out_len) };
    OK
}

// MARK: Hardening

/// The user's SID as text («S-1-5-21-…»).
fn user_sid() -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut size = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        let mut buf = vec![0u8; size as usize];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), size, &mut size);
        let _ = CloseHandle(token);
        got.ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut string = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut string).ok()?;
        let sid = string.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(string.0 as *mut c_void)));
        sid
    }
}

/// A security descriptor from SDDL; the caller LocalFrees it.
fn descriptor(sddl: &str) -> Option<PSECURITY_DESCRIPTOR> {
    let text = wide(sddl);
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(text.as_ptr()), SDDL_REVISION_1, &mut sd, None) }.ok()?;
    Some(sd)
}

fn dacl(sd: PSECURITY_DESCRIPTOR) -> Option<*mut ACL> {
    let (mut present, mut defaulted) = (BOOL(0), BOOL(0));
    let mut acl: *mut ACL = std::ptr::null_mut();
    unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) }.ok()?;
    Some(acl)
}

/// Who may open the signer's process: SYSTEM fully; its own user (and the owner's implicit rights, «OW») only to see
/// it, wait for it and end it. No reading or writing its memory, no new threads in it, no changing this rule — for
/// programs of the same user. An administrator's debug right still passes, as root does on a Mac.
fn process_sddl(sid: &str) -> String {
    let limited = PROCESS_QUERY_LIMITED_INFORMATION.0 | PROCESS_SYNCHRONIZE.0 | PROCESS_TERMINATE.0;
    format!("D:P(A;;GA;;;SY)(A;;{limited:#x};;;OW)(A;;{limited:#x};;;{sid})")
}

/// The same for threads, so no one can stop one and point it at other code.
fn thread_sddl(sid: &str) -> String {
    let limited = THREAD_QUERY_LIMITED_INFORMATION.0 | THREAD_SYNCHRONIZE.0;
    format!("D:P(A;;GA;;;SY)(A;;{limited:#x};;;OW)(A;;{limited:#x};;;{sid})")
}

/// Hardens the calling process (the signer). Returns 0, or a bit for each step that failed: 1 the process's access
/// rule, 2 its threads', 4 the rule for threads to come, 8 the mitigations (no extension DLLs, no remote images,
/// no child processes).
#[unsafe(no_mangle)]
pub extern "C" fn winsec_harden() -> i32 {
    let mut failed = 0;
    let Some(sid) = user_sid() else { return 1 | 2 | 4 };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        match descriptor(&process_sddl(&sid)) {
            Some(sd) => {
                let set = dacl(sd).map(|acl| {
                    SetSecurityInfo(GetCurrentProcess(), SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                                    None, None, Some(acl), None)
                });
                if set != Some(ERROR_SUCCESS) { failed |= 1 }
                let _ = LocalFree(Some(HLOCAL(sd.0)));
            }
            None => failed |= 1,
        }
        match descriptor(&thread_sddl(&sid)) {
            Some(sd) => {
                if let Some(acl) = dacl(sd) {
                    // Threads to come take the token's default rule.
                    let mut token = HANDLE::default();
                    let ok = OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_DEFAULT | TOKEN_QUERY, &mut token).is_ok() && {
                        let info = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
                        let r = SetTokenInformation(token, TokenDefaultDacl, &info as *const _ as *const c_void, size_of::<TOKEN_DEFAULT_DACL>() as u32);
                        let _ = CloseHandle(token);
                        r.is_ok()
                    };
                    if !ok { failed |= 4 }
                    // Threads already running get it one by one.
                    if !harden_threads(acl) { failed |= 2 }
                } else {
                    failed |= 2 | 4;
                }
                let _ = LocalFree(Some(HLOCAL(sd.0)));
            }
            None => failed |= 2 | 4,
        }
        // DisableExtensionPoints; NoRemoteImages | NoLowMandatoryLabelImages | PreferSystem32Images; NoChildProcessCreation.
        let policies: [(PROCESS_MITIGATION_POLICY, u32); 3] =
            [(ProcessExtensionPointDisablePolicy, 1), (ProcessImageLoadPolicy, 0b111), (ProcessChildProcessPolicy, 1)];
        for (policy, flags) in policies {
            if SetProcessMitigationPolicy(policy, &flags as *const u32 as *const c_void, size_of::<u32>()).is_err() { failed |= 8 }
        }
    }
    failed
}

unsafe fn harden_threads(acl: *mut ACL) -> bool {
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return false };
        let me = GetCurrentProcessId();
        let mut entry = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut all = true;
        let mut more = Thread32First(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32OwnerProcessID == me {
                match OpenThread(THREAD_ACCESS_RIGHTS(0x0004_0000 /* WRITE_DAC */), false, entry.th32ThreadID) {
                    Ok(thread) => {
                        let r = SetSecurityInfo(thread, SE_KERNEL_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                                                None, None, Some(acl), None);
                        if r != ERROR_SUCCESS { all = false }
                        let _ = CloseHandle(thread);
                    }
                    // Gone since the snapshot: nothing to protect.
                    Err(e) if e.code() == ERROR_INVALID_PARAMETER.to_hresult() => {}
                    // Born with the rule already (the main thread, from winsec_spawn_signer; later ones from the token's
                    // default): its OWNER RIGHTS entry takes WRITE_DAC from us too. It counts as protected only if what
                    // the rule is for — pointing it at other code, stopping it — is refused to us as well.
                    Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                        match OpenThread(THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME | THREAD_SET_INFORMATION, false, entry.th32ThreadID) {
                            Ok(thread) => { all = false; let _ = CloseHandle(thread); }
                            Err(e) => if e.code() != ERROR_ACCESS_DENIED.to_hresult() && e.code() != ERROR_INVALID_PARAMETER.to_hresult() { all = false },
                        }
                    }
                    Err(_) => all = false,
                }
            }
            more = Thread32Next(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        all
    }
}

// MARK: Who started the signer

fn image_path(process: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 32768];
    let mut size = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size) }.ok()?;
    String::from_utf16(&buf[..size as usize]).ok()
}

fn created(process: HANDLE) -> Option<u64> {
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) }.ok()?;
    Some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
}

/// The SHA-256 of the certificate that signed `path`, when its Authenticode signature is valid; None when it is
/// unsigned or the signature doesn't hold.
fn signer_certificate(path: &str) -> Option<[u8; 32]> {
    let file = wide(path);
    let mut info = WINTRUST_FILE_INFO { cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32, pcwszFilePath: PCWSTR(file.as_ptr()), ..Default::default() };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let verdict = unsafe { WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as *mut c_void) };
    let mut thumbprint = None;
    if verdict == 0 {
        unsafe {
            let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
            let signer = if provider.is_null() { std::ptr::null_mut() } else { WTHelperGetProvSignerFromChain(provider, 0, false, 0) };
            let cert = if signer.is_null() { std::ptr::null_mut() } else { WTHelperGetProvCertFromChain(signer, 0) };
            if !cert.is_null() && !(*cert).pCert.is_null() {
                let c = &*(*cert).pCert;
                let encoded = std::slice::from_raw_parts(c.pbCertEncoded, c.cbCertEncoded as usize);
                thumbprint = sha256(encoded);
            }
        }
    }
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe { WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as *mut c_void) };
    thumbprint
}

fn sha256(bytes: &[u8]) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut out) }.ok().ok()?;
    Some(out)
}

/// 0 when the process that started this one is still the one that did (started earlier), its program is
/// `expected` in this program's own folder, and — when this program carries a valid Authenticode signature — it is
/// signed with the same certificate. WRONG_PARENT otherwise. In a per-machine install that folder is writable by
/// administrators only, so a program of the user can't put its own file there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_parent_check(expected: *const u8, expected_len: usize) -> i32 {
    let expected = unsafe { text(expected, expected_len) }.to_lowercase();
    let mut info = PROCESS_BASIC_INFORMATION::default();
    let mut size = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(GetCurrentProcess(), ProcessBasicInformation, &mut info as *mut _ as *mut c_void,
                                  size_of::<PROCESS_BASIC_INFORMATION>() as u32, &mut size)
    };
    if status.is_err() { return WRONG_PARENT }
    let Ok(parent) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, info.InheritedFromUniqueProcessId as u32) }) else {
        return WRONG_PARENT;
    };
    let me = unsafe { GetCurrentProcess() };
    let verdict = (|| {
        let (Some(theirs), Some(mine)) = (image_path(parent), image_path(me)) else { return WRONG_PARENT };
        // A parent that exited and gave its number to a newer process isn't the one that started this.
        match (created(parent), created(me)) {
            (Some(p), Some(m)) if p <= m => {}
            _ => return WRONG_PARENT,
        }
        let folder = |p: &str| p.rsplit_once('\\').map(|(dir, file)| (dir.to_lowercase(), file.to_lowercase()));
        let (Some((their_dir, their_file)), Some((my_dir, _))) = (folder(&theirs), folder(&mine)) else { return WRONG_PARENT };
        if their_dir != my_dir || their_file != expected { return WRONG_PARENT }
        match signer_certificate(&mine) {
            Some(cert) if signer_certificate(&theirs) != Some(cert) => WRONG_PARENT,
            _ => OK,
        }
    })();
    let _ = unsafe { CloseHandle(parent) };
    verdict
}

/// 0 when this program's folder can't be written by its user (a per-machine install); UNAVAILABLE when it can.
#[unsafe(no_mangle)]
pub extern "C" fn winsec_folder_protected() -> i32 {
    let Some(mine) = image_path(unsafe { GetCurrentProcess() }) else { return FAILED };
    let Some((dir, _)) = mine.rsplit_once('\\') else { return FAILED };
    let probe = format!("{dir}\\.ihatecrypto-write-check-{}", std::process::id());
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            UNAVAILABLE
        }
        Err(_) => OK,
    }
}

// MARK: Starting the signer (the engine)

/// Starts `exe` with pipes on its standard input and output, its process and first thread created with the signer's
/// access rule (no gap before it hardens itself). Hands back the process id, a handle that can only wait for it, and
/// this side's ends of the two pipes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_spawn_signer(
    exe: *const u8, exe_len: usize, pid: *mut u32, process: *mut isize, to_child: *mut isize, from_child: *mut isize,
) -> i32 {
    let exe = unsafe { text(exe, exe_len) };
    let Some(sid) = user_sid() else { return FAILED };
    let (Some(psd), Some(tsd)) = (descriptor(&process_sddl(&sid)), descriptor(&thread_sddl(&sid))) else { return FAILED };
    let result = unsafe { spawn(&exe, psd, tsd) };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(psd.0)));
        let _ = LocalFree(Some(HLOCAL(tsd.0)));
    }
    match result {
        Some((id, waiter, input, output)) => {
            unsafe {
                *pid = id;
                *process = waiter.0 as isize;
                *to_child = input.0 as isize;
                *from_child = output.0 as isize;
            }
            OK
        }
        None => FAILED,
    }
}

unsafe fn spawn(exe: &str, psd: PSECURITY_DESCRIPTOR, tsd: PSECURITY_DESCRIPTOR) -> Option<(u32, HANDLE, HANDLE, HANDLE)> {
    unsafe {
        let inherit = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, bInheritHandle: TRUE, ..Default::default() };
        let (mut child_in, mut our_in, mut our_out, mut child_out) = (HANDLE::default(), HANDLE::default(), HANDLE::default(), HANDLE::default());
        CreatePipe(&mut child_in, &mut our_in, Some(&inherit), 0).ok()?;
        CreatePipe(&mut our_out, &mut child_out, Some(&inherit), 0).ok()?;
        // Only the child's ends go to it.
        SetHandleInformation(our_in, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).ok()?;
        SetHandleInformation(our_out, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).ok()?;
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: child_in,
            hStdOutput: child_out,
            hStdError: GetStdHandle(STD_ERROR_HANDLE).unwrap_or_default(),
            ..Default::default()
        };
        let process_attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: psd.0, bInheritHandle: FALSE };
        let thread_attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: tsd.0, bInheritHandle: FALSE };
        let mut line = wide(&format!("\"{exe}\""));
        let app = wide(exe);
        let mut info = PROCESS_INFORMATION::default();
        let started = CreateProcessW(PCWSTR(app.as_ptr()), Some(PWSTR(line.as_mut_ptr())), Some(&process_attributes), Some(&thread_attributes),
                                     true, CREATE_NO_WINDOW, None, PCWSTR::null(), &startup, &mut info);
        let _ = CloseHandle(child_in);
        let _ = CloseHandle(child_out);
        if started.is_err() {
            let _ = CloseHandle(our_in);
            let _ = CloseHandle(our_out);
            return None;
        }
        let _ = CloseHandle(info.hThread);
        // The full-rights handle CreateProcess gave this side goes; only waiting on it stays.
        let mut waiter = HANDLE::default();
        let me = GetCurrentProcess();
        let dup = DuplicateHandle(me, info.hProcess, me, &mut waiter, (PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION).0, false,
                                  DUPLICATE_HANDLE_OPTIONS(0));
        let _ = CloseHandle(info.hProcess);
        dup.ok()?;
        Some((info.dwProcessId, waiter, our_in, our_out))
    }
}

// MARK: Pipes

/// This process's standard input and output, for the signer's side of the pipes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_stdio(input: *mut isize, output: *mut isize) -> i32 {
    unsafe {
        let (Ok(i), Ok(o)) = (GetStdHandle(STD_INPUT_HANDLE), GetStdHandle(STD_OUTPUT_HANDLE)) else { return FAILED };
        *input = i.0 as isize;
        *output = o.0 as isize;
    }
    OK
}

/// Reads exactly `len` bytes; FAILED when the pipe closes first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_read(handle: isize, buf: *mut u8, len: usize) -> i32 {
    use windows::Win32::Storage::FileSystem::ReadFile;
    let mut done = 0usize;
    while done < len {
        let mut got = 0u32;
        let slice = unsafe { std::slice::from_raw_parts_mut(buf.add(done), len - done) };
        if unsafe { ReadFile(HANDLE(handle as *mut c_void), Some(slice), Some(&mut got), None) }.is_err() || got == 0 { return FAILED }
        done += got as usize;
    }
    OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn winsec_write(handle: isize, buf: *const u8, len: usize) -> i32 {
    use windows::Win32::Storage::FileSystem::WriteFile;
    let mut done = 0usize;
    while done < len {
        let mut put = 0u32;
        let slice = unsafe { std::slice::from_raw_parts(buf.add(done), len - done) };
        if unsafe { WriteFile(HANDLE(handle as *mut c_void), Some(slice), Some(&mut put), None) }.is_err() || put == 0 { return FAILED }
        done += put as usize;
    }
    OK
}

#[unsafe(no_mangle)]
pub extern "C" fn winsec_close(handle: isize) { let _ = unsafe { CloseHandle(HANDLE(handle as *mut c_void)) }; }

/// Lets process `pid` (the signer) bring its window and Windows Hello forward once: the app is in front when the
/// user presses Confirm, and Windows lets only it hand that on.
#[unsafe(no_mangle)]
pub extern "C" fn winsec_allow_foreground(pid: u32) -> i32 {
    if unsafe { AllowSetForegroundWindow(pid) }.is_ok() { OK } else { FAILED }
}

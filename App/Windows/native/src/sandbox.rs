//! The engine's AppContainer, the Windows counterpart of the Mac's App Sandbox. The window makes the container once,
//! lets it into the engine's own data folder, and starts the engine in it with its pipes and nothing else. In there the
//! engine reaches the internet, but not the person's files, the signer's folder, the registry's autostart keys or any
//! other program: it may start no process at all. What it can't do the window does for it, a fixed thing per request
//! (the broker, the window's broker.rs), and hands it only the pipes of what it started (`give`).

use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;

use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::Isolation::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ};
use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::*;
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// The capabilities the engine runs with: connections out to the internet (internetClient) and to the home or office
/// network (privateNetworkClientServer: a person's own node on the LAN). Nothing that listens to the internet, no
/// documents, no devices.
const CAPABILITIES: [&str; 2] = ["S-1-15-3-1", "S-1-15-3-3"];

/// A SID Windows allocated for us.
pub struct Sid(PSID, bool);

// SAFETY: the SID is only read once made, from any thread.
unsafe impl Send for Sid {}
unsafe impl Sync for Sid {}

impl Sid {
    pub fn as_psid(&self) -> PSID { self.0 }

    /// «S-1-15-2-…».
    pub fn text(&self) -> Option<String> {
        let mut out = PWSTR::null();
        unsafe { ConvertSidToStringSidW(self.0, &mut out) }.ok()?;
        let text = unsafe { out.to_string() }.ok();
        unsafe { let _ = LocalFree(Some(HLOCAL(out.0 as *mut c_void))); }
        text
    }

    fn parse(text: &str) -> Option<Sid> {
        let mut sid = PSID::default();
        unsafe { ConvertStringSidToSidW(&HSTRING::from(text), &mut sid) }.ok()?;
        Some(Sid(sid, false))
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        unsafe {
            // Derive/CreateAppContainerProfile's SIDs go back with FreeSid, ConvertStringSidToSid's with LocalFree.
            if self.1 { FreeSid(self.0); } else { let _ = LocalFree(Some(HLOCAL(self.0.0))); }
        }
    }
}

/// The container `name`, made the first time it's asked for. The same name gives the same SID on every PC.
pub fn container(name: &str) -> Option<Sid> {
    let n = HSTRING::from(name);
    match unsafe { CreateAppContainerProfile(&n, &n, &n, None) } {
        Ok(sid) => Some(Sid(sid, true)),
        Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() => unsafe { DeriveAppContainerSidFromAppContainerName(&n) }.ok().map(|s| Sid(s, true)),
        Err(_) => None,
    }
}

/// The container's SID without making it (the installer's loopback rule, a check).
pub fn container_sid(name: &str) -> Option<Sid> {
    unsafe { DeriveAppContainerSidFromAppContainerName(&HSTRING::from(name)) }.ok().map(|s| Sid(s, true))
}

/// The container's own folder (%LOCALAPPDATA%\Packages\<name>\AC), which only it and its user may write: Windows
/// gives the process in it this folder as its LOCALAPPDATA, and its Temp inside as TEMP.
pub fn folder(sid: &Sid) -> Option<std::path::PathBuf> {
    let path = unsafe { GetAppContainerFolderPath(&HSTRING::from(sid.text()?)) }.ok()?;
    let text = unsafe { path.to_string() }.ok();
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const c_void)) };
    text.map(Into::into)
}

/// Lets `sid` into `path` and everything in it, files to come included: all of it with `write`, else only reading and
/// running. Added to what's there (the person keeps theirs). False when Windows refused — a folder the person can't
/// change the rules of, such as Program Files, which already lets every container read and run.
pub fn grant(path: &Path, sid: &Sid, write: bool) -> bool {
    let name = HSTRING::from(path.as_os_str());
    let (mut dacl, mut sd) = (std::ptr::null_mut::<ACL>(), PSECURITY_DESCRIPTOR::default());
    if unsafe { GetNamedSecurityInfoW(&name, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, None, None, Some(&mut dacl), None, &mut sd) } != ERROR_SUCCESS {
        return false;
    }
    let access = if write { FILE_ALL_ACCESS.0 } else { (FILE_GENERIC_READ | FILE_GENERIC_EXECUTE).0 };
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: PWSTR(sid.0.0 as *mut u16),
        },
    };
    let mut merged = std::ptr::null_mut::<ACL>();
    let ok = unsafe { SetEntriesInAclW(Some(&[entry]), Some(dacl), &mut merged) } == ERROR_SUCCESS
        && unsafe { SetNamedSecurityInfoW(&name, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, None, None, Some(merged), None) } == ERROR_SUCCESS;
    unsafe {
        if !merged.is_null() { let _ = LocalFree(Some(HLOCAL(merged as *mut c_void))); }
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    ok
}

/// The engine started in its container: its id and process handle, its standard input and output, and the broker's
/// two pipes — its requests in, the answers out.
pub struct Contained {
    pub pid: u32,
    pub process: OwnedHandle,
    pub stdin: File,
    pub stdout: File,
    pub requests: File,
    pub answers: File,
}

fn pipe() -> std::io::Result<(HANDLE, HANDLE)> {
    let inherit = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, bInheritHandle: TRUE, ..Default::default() };
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    unsafe { CreatePipe(&mut read, &mut write, Some(&inherit), 0) }?;
    Ok((read, write))
}

fn not_inherited(h: HANDLE) -> std::io::Result<()> {
    unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }?;
    Ok(())
}

/// Starts `exe` in the container `sid` with `env` added to this process's environment and four pipes: its standard
/// input and output, and the broker's (their handle numbers in IHATECRYPTO_BROKER, «requests,answers»). It inherits
/// those and this process's standard error only, and may start no process of its own.
pub fn spawn(exe: &Path, env: &[(&str, String)], sid: &Sid) -> std::io::Result<Contained> {
    let (stdin_read, stdin_write) = pipe()?;
    let (stdout_read, stdout_write) = pipe()?;
    let (requests_read, requests_write) = pipe()?;
    let (answers_read, answers_write) = pipe()?;
    let ours = [stdin_write, stdout_read, requests_read, answers_write];
    let theirs = [stdin_read, stdout_write, requests_write, answers_read];
    let close = |hs: &[HANDLE]| for h in hs { unsafe { let _ = CloseHandle(*h); } };
    let started = (|| -> std::io::Result<PROCESS_INFORMATION> {
        for h in ours { not_inherited(h)? }
        // Standard error as this process has it (a console in a development run), an inheritable copy of its own.
        let mut stderr = HANDLE::default();
        if let Ok(err) = unsafe { GetStdHandle(STD_ERROR_HANDLE) } && !err.is_invalid() && !err.0.is_null() {
            let me = unsafe { GetCurrentProcess() };
            if unsafe { DuplicateHandle(me, err, me, &mut stderr, 0, true, DUPLICATE_SAME_ACCESS) }.is_err() { stderr = HANDLE::default() }
        }
        let mut inherited: Vec<HANDLE> = theirs.to_vec();
        if !stderr.is_invalid() && !stderr.0.is_null() { inherited.push(stderr) }

        let mut block: Vec<u16> = Vec::new();
        let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
            .filter(|(k, _)| !env.iter().any(|(n, _)| k.eq_ignore_ascii_case(n)) && !k.eq_ignore_ascii_case("IHATECRYPTO_BROKER")).collect();
        vars.extend(env.iter().map(|(k, v)| ((*k).into(), v.into())));
        vars.push(("IHATECRYPTO_BROKER".into(), format!("{},{}", requests_write.0 as usize, answers_read.0 as usize).into()));
        for (k, v) in vars {
            use std::os::windows::ffi::OsStrExt;
            block.extend(k.encode_wide());
            block.push('=' as u16);
            block.extend(v.encode_wide());
            block.push(0);
        }
        block.push(0);

        let mut capabilities: Vec<(Sid, SID_AND_ATTRIBUTES)> = CAPABILITIES.iter().filter_map(|c| Sid::parse(c)).map(|s| {
            let attrs = SID_AND_ATTRIBUTES { Sid: s.0, Attributes: windows::Win32::System::SystemServices::SE_GROUP_ENABLED as u32 };
            (s, attrs)
        }).collect();
        let mut list: Vec<SID_AND_ATTRIBUTES> = capabilities.iter_mut().map(|(_, a)| *a).collect();
        let security = SECURITY_CAPABILITIES {
            AppContainerSid: sid.0, Capabilities: list.as_mut_ptr(), CapabilityCount: list.len() as u32, Reserved: 0,
        };
        // PROCESS_CREATION_CHILD_PROCESS_RESTRICTED: CreateProcess fails in it, and in anything it could start.
        let no_children: u32 = 1;

        let mut size = 0usize;
        let _ = unsafe { InitializeProcThreadAttributeList(None, 3, None, &mut size) };
        let mut buffer = vec![0u8; size];
        let attributes = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr() as *mut c_void);
        unsafe { InitializeProcThreadAttributeList(Some(attributes), 3, None, &mut size) }?;
        let result = (|| -> std::io::Result<PROCESS_INFORMATION> {
            unsafe {
                UpdateProcThreadAttribute(attributes, 0, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize, Some(&security as *const _ as *const c_void),
                                          size_of::<SECURITY_CAPABILITIES>(), None, None)?;
                UpdateProcThreadAttribute(attributes, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, Some(inherited.as_ptr() as *const c_void),
                                          inherited.len() * size_of::<HANDLE>(), None, None)?;
                UpdateProcThreadAttribute(attributes, 0, PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY as usize, Some(&no_children as *const u32 as *const c_void),
                                          size_of::<u32>(), None, None)?;
            }
            let startup = STARTUPINFOEXW {
                StartupInfo: STARTUPINFOW {
                    cb: size_of::<STARTUPINFOEXW>() as u32,
                    dwFlags: STARTF_USESTDHANDLES,
                    hStdInput: stdin_read,
                    hStdOutput: stdout_write,
                    hStdError: stderr,
                    ..Default::default()
                },
                lpAttributeList: attributes,
            };
            let mut line: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
            let app = HSTRING::from(exe.as_os_str());
            let mut info = PROCESS_INFORMATION::default();
            unsafe {
                CreateProcessW(&app, Some(PWSTR(line.as_mut_ptr())), None, None, true,
                               EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | DETACHED_PROCESS, Some(block.as_ptr() as *const c_void),
                               PCWSTR::null(), &startup.StartupInfo, &mut info)?;
            }
            Ok(info)
        })();
        unsafe { DeleteProcThreadAttributeList(attributes) };
        if !stderr.is_invalid() && !stderr.0.is_null() { close(&[stderr]) }
        drop(capabilities);
        result
    })();
    close(&theirs);
    let info = match started {
        Ok(info) => info,
        Err(e) => { close(&ours); return Err(e) }
    };
    unsafe { let _ = CloseHandle(info.hThread); }
    // SAFETY: fresh handles that only these values own from here.
    let file = |h: HANDLE| unsafe { File::from_raw_handle(h.0 as RawHandle) };
    Ok(Contained {
        pid: info.dwProcessId,
        process: unsafe { OwnedHandle::from_raw_handle(info.hProcess.0 as RawHandle) },
        stdin: file(stdin_write),
        stdout: file(stdout_read),
        requests: file(requests_read),
        answers: file(answers_write),
    })
}

/// Hands `handle` to the process `target` (the contained engine), closing it here: its number there, or None. With
/// `access` only those rights go (a process that can only be waited for and ended), else the handle's own.
pub fn give(target: RawHandle, handle: OwnedHandle, access: Option<u32>) -> Option<usize> {
    use std::os::windows::io::IntoRawHandle;
    let source = HANDLE(handle.into_raw_handle() as *mut c_void);
    let mut theirs = HANDLE::default();
    let options = DUPLICATE_CLOSE_SOURCE | if access.is_none() { DUPLICATE_SAME_ACCESS } else { DUPLICATE_HANDLE_OPTIONS(0) };
    unsafe { DuplicateHandle(GetCurrentProcess(), source, HANDLE(target as *mut c_void), &mut theirs, access.unwrap_or(0), false, options) }.ok()?;
    Some(theirs.0 as usize)
}

/// Whether this process runs in an AppContainer.
pub fn contained() -> bool {
    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() { return false }
    let mut is = 0u32;
    let mut size = 0u32;
    let ok = unsafe { GetTokenInformation(token, TokenIsAppContainer, Some(&mut is as *mut u32 as *mut c_void), 4, &mut size) }.is_ok();
    unsafe { let _ = CloseHandle(token); }
    ok && is != 0
}

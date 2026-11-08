//! What a process in the engine's AppContainer can and can't reach, checked on a real Windows:
//!   acprobe run [container]   starts itself in the container (default: the release engine's) and prints each check
//!   acprobe token <pid>       whether a running process (the engine) is in an AppContainer, and which, and its level
//! The checks run inside: the person's Documents, the signer's folder, the autostart keys and folder, Task Scheduler,
//! starting a program, DPAPI, Windows Hello, the session's lock and idle time, the internet, loopback both ways, and
//! HID devices (a Ledger's way in).

#[cfg(not(windows))]
fn main() { eprintln!("acprobe runs on Windows only") }

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if std::env::var("ACPROBE_CHILD").is_ok() { return child() }
    match args.get(1).map(String::as_str) {
        Some("run") => run(args.get(2).map(String::as_str).unwrap_or("app.ihatecrypto.windows.engine")),
        Some("token") => token(args.get(2).and_then(|p| p.parse().ok()).expect("acprobe token <pid>")),
        _ => eprintln!("acprobe run [container] | acprobe token <pid>"),
    }
}

#[cfg(windows)]
fn env(k: &str) -> String { std::env::var(k).unwrap_or_default() }

#[cfg(windows)]
fn run(name: &str) {
    use std::io::{BufRead, BufReader, Read, Write};
    let docs = std::path::Path::new(&env("USERPROFILE")).join("Documents").join("acprobe-secret.txt");
    std::fs::write(&docs, "secret").expect("Documents");
    let blob = {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let plain = b"dpapi-ok";
        assert_eq!(unsafe { winsec::winsec_protect(plain.as_ptr(), plain.len(), &mut out, &mut len) }, 0);
        let hex: String = unsafe { std::slice::from_raw_parts(out, len) }.iter().map(|b| format!("{b:02x}")).collect();
        unsafe { winsec::winsec_free(out, len) };
        hex
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || for c in listener.incoming().flatten() { let mut c = c; let _ = c.write_all(b"hello"); });
    let data = std::path::Path::new(&env("LOCALAPPDATA")).join("acprobe-data");
    std::fs::create_dir_all(&data).unwrap();
    let sid = winsec::sandbox::container(name).expect("the container");
    println!("container {name} = {}", sid.text().unwrap_or_default());
    println!("grant data folder: {}", winsec::sandbox::grant(&data, &sid, true));
    let exe = std::env::current_exe().unwrap();
    println!("grant own folder: {}", winsec::sandbox::grant(exe.parent().unwrap(), &sid, false));
    let signer = std::path::Path::new(&env("LOCALAPPDATA")).join("ihatecrypto").join("signer");
    let _ = std::fs::create_dir_all(&signer);
    let _ = std::fs::write(signer.join("acprobe-victim.txt"), "x");
    let vars = [("ACPROBE_CHILD", "1".to_string()), ("ACPROBE_PORT", port.to_string()), ("ACPROBE_BLOB", blob),
                ("ACPROBE_DATA", data.display().to_string()), ("ACPROBE_SIGNER", signer.display().to_string()),
                ("ACPROBE_DOC", docs.display().to_string())];
    let mut child = match winsec::sandbox::spawn(&exe, &vars, &sid) {
        Ok(c) => c,
        Err(e) => { println!("spawn failed: {e}"); return }
    };
    println!("spawned pid {}", child.pid);
    // The broker's pipes: the child says hello on its requests pipe, and reads an answer.
    let mut requests = BufReader::new(child.requests);
    std::thread::spawn(move || { let mut line = String::new(); let _ = requests.read_line(&mut line); println!("broker pipe got: {}", line.trim()); });
    let _ = writeln!(child.answers, "answer");
    drop(child.stdin);
    for line in BufReader::new(child.stdout).lines() {
        let Ok(line) = line else { break };
        println!("{line}");
        if let Some(p) = line.strip_prefix("LISTEN ") {
            let r = std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{p}").parse().unwrap(), std::time::Duration::from_secs(3))
                .and_then(|mut s| { s.set_read_timeout(Some(std::time::Duration::from_secs(3)))?; let mut b = [0u8; 2]; s.read_exact(&mut b).map(|_| b) });
            println!("(outside) connect to its listener: {}", if r.is_ok() { "OK".to_string() } else { format!("FAILED {:?}", r.err()) });
        }
    }
    let mut code = 0u32;
    unsafe {
        use std::os::windows::io::AsRawHandle;
        let h = windows::Win32::Foundation::HANDLE(child.process.as_raw_handle());
        let _ = windows::Win32::System::Threading::WaitForSingleObject(h, 5000);
        let _ = windows::Win32::System::Threading::GetExitCodeProcess(h, &mut code);
    }
    println!("child exit code 0x{code:x}");
    let _ = std::fs::remove_file(&docs);
    println!("victim still there: {}", signer.join("acprobe-victim.txt").exists());
    let _ = std::fs::remove_file(signer.join("acprobe-victim.txt"));
}

#[cfg(windows)]
fn say(what: &str, r: Result<String, String>) {
    match r { Ok(d) => println!("{what}: ALLOWED {d}"), Err(e) => println!("{what}: refused ({e})") }
}

#[cfg(windows)]
fn child() {
    use std::io::{BufRead, Read, Write};
    use std::time::Duration;
    println!("contained: {}", winsec::sandbox::contained());
    // The broker's pipes from the environment.
    if let Some((r, a)) = env("IHATECRYPTO_BROKER").split_once(',') {
        use std::os::windows::io::FromRawHandle;
        let (r, a): (usize, usize) = (r.parse().unwrap_or(0), a.parse().unwrap_or(0));
        let mut requests = unsafe { std::fs::File::from_raw_handle(r as _) };
        let answers = unsafe { std::fs::File::from_raw_handle(a as _) };
        let _ = writeln!(requests, "hello from inside");
        let mut line = String::new();
        let _ = std::io::BufReader::new(answers).read_line(&mut line);
        println!("broker answer read: {}", line.trim());
    }
    say("read Documents file", std::fs::read_to_string(env("ACPROBE_DOC")).map_err(|e| e.to_string()));
    say("list Documents", std::fs::read_dir(std::path::Path::new(&env("USERPROFILE")).join("Documents")).map(|d| format!("{} entries", d.count())).map_err(|e| e.to_string()));
    say("read signer folder", std::fs::read_dir(env("ACPROBE_SIGNER")).map(|d| format!("{} entries", d.count())).map_err(|e| e.to_string()));
    say("write signer folder", std::fs::write(std::path::Path::new(&env("ACPROBE_SIGNER")).join("acprobe.txt"), "x").map(|_| String::new()).map_err(|e| e.to_string()));
    for k in ["LOCALAPPDATA", "APPDATA", "USERPROFILE", "TEMP"] { println!("{k}={}", env(k)) }
    say("delete a file in the signer folder", std::fs::remove_file(std::path::Path::new(&env("ACPROBE_SIGNER")).join("acprobe-victim.txt")).map(|_| String::new()).map_err(|e| e.to_string()));
    let data = std::path::Path::new(&env("ACPROBE_DATA")).join("probe.txt");
    say("write own data folder", std::fs::write(&data, "x").and_then(|_| std::fs::read_to_string(&data)).map_err(|e| e.to_string()));
    say("temp dir write", std::fs::write(std::env::temp_dir().join("acprobe.txt"), "x").map(|_| std::env::temp_dir().display().to_string()).map_err(|e| e.to_string()));
    let startup = std::path::Path::new(&env("APPDATA")).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup\\acprobe.cmd");
    say("write Startup folder", std::fs::write(&startup, "echo x").map(|_| String::new()).map_err(|e| e.to_string()));
    say("write HKCU Run key", run_key());
    say("start cmd.exe", std::process::Command::new("C:\\Windows\\System32\\cmd.exe").args(["/c", "exit"]).status().map(|s| format!("{s}")).map_err(|e| e.to_string()));
    // DPAPI: a blob made outside, and one made here.
    let blob: Vec<u8> = (0..env("ACPROBE_BLOB").len() / 2).map(|i| u8::from_str_radix(&env("ACPROBE_BLOB")[2 * i..2 * i + 2], 16).unwrap()).collect();
    say("DPAPI unprotect outside blob", {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let code = unsafe { winsec::winsec_unprotect(blob.as_ptr(), blob.len(), &mut out, &mut len) };
        if code == 0 { Ok(String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(out, len) }).into_owned()) } else { Err(format!("code {code}")) }
    });
    say("DPAPI protect here", {
        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
        let code = unsafe { winsec::winsec_protect(b"x".as_ptr(), 1, &mut out, &mut len) };
        if code == 0 { Ok(format!("{len} bytes")) } else { Err(format!("code {code}")) }
    });
    println!("Windows Hello available: {}", winsec::winsec_hello_available() == 0);
    println!("session locked (OpenInputDesktop): {}", winsec::winsec_session_locked());
    println!("idle ms: {}", winsec::winsec_idle_ms());
    say("internet connect 1.1.1.1:443", std::net::TcpStream::connect_timeout(&"1.1.1.1:443".parse().unwrap(), Duration::from_secs(5)).map(|_| String::new()).map_err(|e| e.to_string()));
    say("DNS + connect example.com:443", std::net::ToSocketAddrs::to_socket_addrs(&("example.com", 443)).map_err(|e| e.to_string())
        .and_then(|mut a| a.next().ok_or("no address".into())).and_then(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_secs(5)).map(|_| a.to_string()).map_err(|e| e.to_string())));
    say("loopback connect to outside listener", std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{}", env("ACPROBE_PORT")).parse().unwrap(), Duration::from_secs(3))
        .and_then(|mut s| { s.set_read_timeout(Some(Duration::from_secs(3)))?; let mut b = [0u8; 5]; s.read_exact(&mut b).map(|_| String::from_utf8_lossy(&b).into_owned()) })
        .map_err(|e| e.to_string()));
    say("HID devices", hid());
    match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(l) => {
            let port = l.local_addr().unwrap().port();
            l.set_nonblocking(true).unwrap();
            println!("LISTEN {port}");
            let _ = std::io::stdout().flush();
            let start = std::time::Instant::now();
            let got = loop {
                match l.accept() {
                    Ok((mut s, _)) => { let _ = s.write_all(b"ok"); break true }
                    Err(_) if start.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(50)),
                    Err(_) => break false,
                }
            };
            println!("(inside) its listener accepted: {got}");
        }
        Err(e) => println!("bind loopback: refused ({e})"),
    }
    let _ = std::io::stdout().flush();
    say("Task Scheduler connect", scheduler());
}

#[cfg(windows)]
fn run_key() -> Result<String, String> {
    use windows::Win32::System::Registry::*;
    use windows::core::w;
    let mut key = HKEY::default();
    let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"), None, KEY_SET_VALUE, &mut key) };
    if r.is_err() { return Err(format!("{r:?}")) }
    let value: Vec<u16> = "x".encode_utf16().chain(Some(0)).collect();
    let set = unsafe { RegSetValueExW(key, w!("acprobe"), None, REG_SZ, Some(std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2))) };
    if set.is_ok() { let _ = unsafe { RegDeleteValueW(key, w!("acprobe")) }; }
    let _ = unsafe { RegCloseKey(key) };
    if set.is_ok() { Ok("set".into()) } else { Err(format!("{set:?}")) }
}

#[cfg(windows)]
fn scheduler() -> Result<String, String> {
    use windows::Win32::System::Com::*;
    use windows::Win32::System::TaskScheduler::*;
    use windows::Win32::System::Variant::VARIANT;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty).map_err(|e| e.to_string())?;
        let folder = service.GetFolder(&windows::core::BSTR::from("\\")).map_err(|e| e.to_string())?;
        let tasks = folder.GetTasks(1).map_err(|e| e.to_string())?;
        Ok(format!("connected, {} tasks visible", tasks.Count().unwrap_or(-1)))
    }
}

#[cfg(windows)]
fn hid() -> Result<String, String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::*;
    use windows::core::PCWSTR;
    unsafe {
        let guid = HidD_GetHidGuid();
        let set = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE).map_err(|e| e.to_string())?;
        let (mut found, mut opened) = (0, 0);
        let mut i = 0;
        loop {
            let mut data = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInterfaces(set, None, &guid, i, &mut data).is_err() { break }
            i += 1;
            found += 1;
            let mut needed = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &data, None, 0, Some(&mut needed), None);
            let mut buf = vec![0u32; (needed as usize).div_ceil(4)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &data, Some(detail), needed, None, None).is_err() { continue }
            if let Ok(h) = CreateFileW(PCWSTR((*detail).DevicePath.as_ptr()), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) {
                opened += 1;
                let _ = CloseHandle(h);
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
        if opened > 0 { Ok(format!("{found} found, {opened} opened")) } else { Err(format!("{found} found, none opened")) }
    }
}

#[cfg(windows)]
fn token(pid: u32) {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::*;
    use windows::Win32::System::Threading::*;
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).expect("open the process");
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).expect("its token");
        let mut is = 0u32;
        let mut size = 0u32;
        let _ = GetTokenInformation(token, TokenIsAppContainer, Some(&mut is as *mut u32 as *mut _), 4, &mut size);
        println!("pid {pid}: AppContainer {}", is != 0);
        let mut buf = vec![0u8; 256];
        if GetTokenInformation(token, TokenAppContainerSid, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, &mut size).is_ok() {
            let sid = (*(buf.as_ptr() as *const TOKEN_APPCONTAINER_INFORMATION)).TokenAppContainer;
            if !sid.0.is_null() {
                let mut text = windows::core::PWSTR::null();
                let _ = windows::Win32::Security::Authorization::ConvertSidToStringSidW(sid, &mut text);
                println!("container SID {}", text.to_string().unwrap_or_default());
            }
        }
        let mut level = vec![0u8; 64];
        if GetTokenInformation(token, TokenIntegrityLevel, Some(level.as_mut_ptr() as *mut _), level.len() as u32, &mut size).is_ok() {
            let label = &*(level.as_ptr() as *const TOKEN_MANDATORY_LABEL);
            let count = *GetSidSubAuthorityCount(label.Label.Sid);
            let rid = *GetSidSubAuthority(label.Label.Sid, count as u32 - 1);
            println!("integrity 0x{rid:x} ({})", match rid { 0x1000 => "low", 0x2000 => "medium", 0x3000 => "high", _ => "other" });
        }
        let mut caps = vec![0u8; 1024];
        if GetTokenInformation(token, TokenCapabilities, Some(caps.as_mut_ptr() as *mut _), caps.len() as u32, &mut size).is_ok() {
            let groups = &*(caps.as_ptr() as *const TOKEN_GROUPS);
            let list = std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize);
            let names: Vec<String> = list.iter().map(|g| {
                let mut text = windows::core::PWSTR::null();
                let _ = windows::Win32::Security::Authorization::ConvertSidToStringSidW(g.Sid, &mut text);
                text.to_string().unwrap_or_default()
            }).collect();
            println!("capabilities {names:?}");
        }
        let mut policy = 0u32;
        if GetProcessMitigationPolicy(process, ProcessChildProcessPolicy, &mut policy as *mut u32 as *mut _, 4).is_ok() {
            println!("no child processes: {}", policy & 1 != 0);
        } else {
            println!("child process policy: unreadable");
        }
        let _ = CloseHandle(token);
        let _ = CloseHandle(process);
    }
}

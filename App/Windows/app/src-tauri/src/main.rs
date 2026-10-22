//! The window: the page in ../ui over the engine. The engine (ihatecrypto-engine, Swift) runs as this process's child
//! with pipes; every line it writes reaches the page as the «engine» event, every line the page sends goes to it. The
//! engine starts the signer itself, so this process never holds a key or talks to the signer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

use tauri::{Emitter, Manager};

struct Engine {
    input: Mutex<ChildStdin>,
    child: Mutex<Child>,
}

/// The engine installed next to the app (resources/bin). A development build may take another one from
/// IHATECRYPTO_ENGINE; a release never does, so nothing set in the user's environment can stand in for it.
fn engine_path(app: &tauri::AppHandle) -> PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("IHATECRYPTO_ENGINE") {
        return path.into();
    }
    let name = if cfg!(windows) { "ihatecrypto-engine.exe" } else { "ihatecrypto-engine" };
    app.path().resource_dir().map(|dir| dir.join("bin").join(name)).unwrap_or_else(|_| name.into())
}

fn start_engine(app: &tauri::AppHandle) -> std::io::Result<Engine> {
    let mut command = Command::new(engine_path(app));
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let input = child.stdin.take().expect("engine stdin");
    let output = child.stdout.take().expect("engine stdout");
    let handle = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            // The signer is about to ask Windows Hello: only the window in front may let it come forward.
            if line.starts_with("{\"event\":\"signerForeground\"") || line.contains("\"event\":\"signerForeground\"") {
                if let Some(pid) = serde_json::from_str::<serde_json::Value>(&line).ok()
                    .and_then(|v| v["data"]["pid"].as_str().and_then(|p| p.parse::<u32>().ok())) {
                    allow_foreground(pid);
                }
            }
            let _ = handle.emit("engine", line);
        }
        let _ = handle.emit("engine", "{\"event\":\"engineStopped\",\"data\":{}}");
    });
    Ok(Engine { input: Mutex::new(input), child: Mutex::new(child) })
}

#[cfg(windows)]
fn allow_foreground(pid: u32) {
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    let _ = unsafe { AllowSetForegroundWindow(pid) };
}

#[cfg(not(windows))]
fn allow_foreground(_pid: u32) {}

/// One request line to the engine.
#[tauri::command]
fn engine_send(state: tauri::State<'_, Engine>, line: String) -> Result<(), String> {
    if line.contains('\n') { return Err("one line at a time".into()) }
    let mut input = state.input.lock().map_err(|_| "the engine is gone")?;
    writeln!(input, "{line}").and_then(|_| input.flush()).map_err(|e| format!("the engine stopped: {e}"))
}

/// While the recovery phrase is on screen the window is left out of screenshots and screen sharing.
#[tauri::command]
fn protect(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_content_protected(on).map_err(|e| e.to_string())
}

/// An explorer link in the default browser: https only, nothing that a shell could read as more than a link.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    let plain = url.starts_with("https://") && url.len() < 512 && url.chars().all(|c| c.is_ascii_graphic() && !"\"'`<>|^%&".contains(c));
    if !plain { return Err("not a plain https link".into()) }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        let target = HSTRING::from(url.as_str());
        unsafe { ShellExecuteW(None, w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Command::new("open").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

/// The recovery phrase to the clipboard, once: kept out of Windows' clipboard history and cloud clipboard, and cleared
/// after 60 seconds unless something else was copied since.
#[tauri::command]
fn copy_secret(text: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        secret_clipboard::copy(&text)
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        Err("only on Windows".into())
    }
}

#[cfg(windows)]
mod secret_clipboard {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    use windows::core::w;

    const CF_UNICODETEXT: u32 = 13;

    unsafe fn put(format: u32, bytes: &[u8]) -> bool {
        unsafe {
            let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
            let target = GlobalLock(memory) as *mut u8;
            if target.is_null() { return false }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
            let _ = GlobalUnlock(memory);
            SetClipboardData(format, Some(HANDLE(memory.0))).is_ok()
        }
    }

    pub fn copy(text: &str) -> Result<(), String> {
        let wide: Vec<u8> = text.encode_utf16().chain(Some(0)).flat_map(|u| u.to_le_bytes()).collect();
        unsafe {
            OpenClipboard(None).map_err(|e| e.to_string())?;
            let _ = EmptyClipboard();
            let ok = put(CF_UNICODETEXT, &wide)
                && put(RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")), &[0])
                && put(RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")), &0u32.to_le_bytes())
                && put(RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")), &0u32.to_le_bytes());
            let _ = CloseClipboard();
            if !ok { return Err("the clipboard refused it".into()) }
            let copied = GetClipboardSequenceNumber();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(60));
                if GetClipboardSequenceNumber() == copied && OpenClipboard(None).is_ok() {
                    let _ = EmptyClipboard();
                    let _ = CloseClipboard();
                }
            });
        }
        Ok(())
    }
}

/// Development builds only: lines of JS appended to the file IHATECRYPTO_UI_EVAL run in the page, so a test can press
/// the window's buttons without taking the screen (the Mac's -launch flags are Debug-only the same way).
#[cfg(debug_assertions)]
fn ui_eval(app: &tauri::AppHandle) {
    let Ok(path) = std::env::var("IHATECRYPTO_UI_EVAL") else { return };
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut done = 0usize;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for line in text.lines().skip(done) {
                if let Some(window) = handle.get_webview_window("main") { let _ = window.eval(line); }
                done += 1;
            }
        }
    });
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let engine = start_engine(app.handle())?;
            app.manage(engine);
            #[cfg(debug_assertions)]
            ui_eval(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![engine_send, protect, open_link, copy_secret])
        .build(tauri::generate_context!())
        .expect("ihatecrypto's window didn't start")
        .run(|app, event| {
            // The engine ends when its input closes; it is stopped outright when the app quits.
            if let tauri::RunEvent::Exit = event {
                if let Some(engine) = app.try_state::<Engine>() {
                    if let Ok(mut child) = engine.child.lock() { let _ = child.kill(); }
                }
            }
        });
}

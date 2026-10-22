//! The window: the page in ../ui over the engine. The engine (ihatecrypto-engine, Swift) runs as this process's child
//! with pipes; every line it writes reaches the page as the «engine» event, every line the page sends goes to it. The
//! engine starts the signer itself, so this process never holds a key or talks to the signer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;

use tauri::{Emitter, Manager};

struct Engine {
    input: Mutex<ChildStdin>,
    child: Mutex<Child>,
    /// The browser's calls waiting for the engine's answer, by request id (from DAPP_IDS up, so the page's own never meet them).
    dapp: Mutex<HashMap<i64, mpsc::Sender<String>>>,
    next: AtomicI64,
}

const DAPP_IDS: i64 = 1_000_000_000;

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
            // An answer to the browser's call goes back to it, never to the main window's page.
            if !line.starts_with("{\"event\"") {
                if let Some(id) = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|v| v["id"].as_i64()).filter(|id| *id >= DAPP_IDS) {
                    if let Some(engine) = handle.try_state::<Engine>() {
                        if let Some(waiter) = engine.dapp.lock().ok().and_then(|mut m| m.remove(&id)) { let _ = waiter.send(line); }
                    }
                    continue;
                }
            }
            // A site's request is on the canvas: the main window comes forward to show it.
            if line.contains("\"event\":\"showMain\"") { show(&handle); }
            // The signer is about to ask Windows Hello: only the window in front may let it come forward.
            if line.starts_with("{\"event\":\"signerForeground\"") || line.contains("\"event\":\"signerForeground\"") {
                if let Some(pid) = serde_json::from_str::<serde_json::Value>(&line).ok()
                    .and_then(|v| v["data"]["pid"].as_str().and_then(|p| p.parse::<u32>().ok())) {
                    allow_foreground(pid);
                    bring_hello_forward();
                }
            }
            // Money that arrived, a price alert, a scheduled move waiting for Confirm: a Windows notification too,
            // since the window may be in the tray.
            if line.contains("\"event\":\"notify\"") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    use tauri_plugin_notification::NotificationExt;
                    let (title, body) = (v["data"]["title"].as_str().unwrap_or("ihatecrypto"), v["data"]["body"].as_str().unwrap_or(""));
                    let _ = handle.notification().builder().title(title).body(body).show();
                }
            }
            // Only to the main window: a page in the browser never hears the engine.
            let _ = handle.emit_to("main", "engine", line);
        }
        let _ = handle.emit_to("main", "engine", "{\"event\":\"engineStopped\",\"data\":{}}");
    });
    Ok(Engine { input: Mutex::new(input), child: Mutex::new(child), dapp: Mutex::new(HashMap::new()), next: AtomicI64::new(DAPP_IDS) })
}

#[cfg(windows)]
fn allow_foreground(pid: u32) {
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    let _ = unsafe { AllowSetForegroundWindow(pid) };
}

#[cfg(not(windows))]
fn allow_foreground(_pid: u32) {}

/// Windows Hello's dialog can open behind this window, which keeps the foreground: while the window is in front it may
/// hand the foreground on, so for a few seconds it brings the dialog forward once it shows.
#[cfg(windows)]
fn bring_hello_forward() {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetForegroundWindow, SetForegroundWindow};
    use windows::core::{PCWSTR, w};
    std::thread::spawn(|| {
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Ok(dialog) = unsafe { FindWindowW(w!("Credential Dialog Xaml Host"), PCWSTR::null()) } {
                if unsafe { GetForegroundWindow() } == dialog { return }
                let _ = unsafe { SetForegroundWindow(dialog) };
            }
        }
    });
}

#[cfg(not(windows))]
fn bring_hello_forward() {}

/// One request line to the engine.
mod voice;

/// The mic: off the main thread, since it waits until the person stops speaking.
#[tauri::command]
async fn listen() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(voice::listen).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn engine_send(state: tauri::State<'_, Engine>, line: String) -> Result<(), String> {
    if line.contains('\n') { return Err("one line at a time".into()) }
    let mut input = state.input.lock().map_err(|_| "the engine is gone")?;
    writeln!(input, "{line}").and_then(|_| input.flush()).map_err(|e| format!("the engine stopped: {e}"))
}

/// ihatecrypto's browser: a window of its own for https sites, with the wallet provider in every page. It shares nothing
/// with the main window's page; its only way to the wallet is `dapp_call`.
// Async: on Windows a webview made inside a synchronous command waits on the event loop that command holds, and the
// window stays white (wry #583).
#[tauri::command]
async fn open_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if url.scheme() != "https" { return Err("only https sites".into()) }
    if let Some(window) = app.get_webview_window("browser") {
        // The same page again only comes forward; another one opens in it.
        if window.url().map(|u| u != url).unwrap_or(true) { window.navigate(url).map_err(|e| e.to_string())?; }
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, "browser", tauri::WebviewUrl::External(url))
        .title("ihatecrypto · browser")
        .inner_size(1180.0, 800.0)
        .initialization_script(include_str!("provider.js"))
        // Pages stay on https; anything else (a file, a custom scheme) isn't opened here.
        .on_navigation(|u| u.scheme() == "https" || u.as_str() == "about:blank")
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// A page's call to the wallet. The origin is the browser window's own address, read here, never what the page says;
/// the engine puts the request on the canvas and answers once the person pressed Confirm or Cancel.
#[tauri::command]
async fn dapp_call(webview: tauri::Webview, state: tauri::State<'_, Engine>, method: String, params: serde_json::Value) -> Result<String, String> {
    if webview.label() != "browser" { return Err("only the browser's pages".into()) }
    const METHODS: [&str; 6] = ["connect", "disconnect", "signMessage", "signTransaction", "signAndSendTransaction", "eth"];
    if !METHODS.contains(&method.as_str()) { return Err(format!("ihatecrypto doesn't support {method}.")) }
    let url = webview.url().map_err(|e| e.to_string())?;
    let (Some(host), "https") = (url.host_str(), url.scheme()) else { return Err("only https sites get the wallet".into()) };
    let origin = format!("https://{host}{}", url.port().map(|p| format!(":{p}")).unwrap_or_default());
    let id = state.next.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    state.dapp.lock().map_err(|_| "the engine is gone")?.insert(id, tx);
    let line = serde_json::json!({ "id": id, "op": "dapp", "name": method, "text": params.to_string(), "address": origin }).to_string();
    {
        let mut input = state.input.lock().map_err(|_| "the engine is gone")?;
        writeln!(input, "{line}").and_then(|_| input.flush()).map_err(|e| format!("the engine stopped: {e}"))?;
    }
    // The person may take a while to read the card.
    let answer = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(900)))
        .await.map_err(|e| e.to_string())?.map_err(|_| "No answer: the request timed out.".to_string())?;
    let v: serde_json::Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["result"]["json"].as_str().unwrap_or("null").to_string())
    } else {
        Err(v["error"].as_str().unwrap_or("Declined.").to_string())
    }
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
                // «browser:» lines go to the browser's page instead; «show:<label>» brings that window forward.
                if let Some(label) = line.strip_prefix("show:") {
                    if let Some(window) = handle.get_webview_window(label) { let _ = window.show(); let _ = window.set_focus(); }
                    done += 1;
                    continue;
                }
                let (label, js) = line.strip_prefix("browser:").map(|js| ("browser", js)).unwrap_or(("main", line));
                if let Some(window) = handle.get_webview_window(label) { let _ = window.eval(js); }
                done += 1;
            }
        }
    });
}

/// The newer version the update feed offers, if any; its archive is checked against the updater key before install.
#[tauri::command]
async fn update_check(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| u.version))
}

/// Downloads the update, checks its signature, runs its installer and restarts.
#[tauri::command]
async fn update_install(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let Some(update) = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())? else { return Ok(()) };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}

fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Closing the window keeps ihatecrypto in the tray, as the Mac's goes to the menu bar: price alerts and the balances
/// go on. Quit from the tray ends it, the engine and the signer with it.
fn tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let open = MenuItem::with_id(app, "open", "Open ihatecrypto", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit ihatecrypto", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::new().tooltip("ihatecrypto").menu(&menu).show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event { show(tray.app_handle()) }
        });
    if let Some(icon) = app.default_window_icon() { builder = builder.icon(icon.clone()); }
    builder.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // Updates come signed with the key in tauri.conf.json (plugins.updater.pubkey); anything else is refused.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let engine = start_engine(app.handle())?;
            app.manage(engine);
            #[cfg(debug_assertions)]
            ui_eval(app.handle());
            tray(app)?;
            // A screen smaller than the window (a small laptop, a VM): it opens maximized instead of reaching under
            // the taskbar.
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
                    if size.width < 1240.0 || size.height < 860.0 { let _ = window.maximize(); }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // The main window closes to the tray; the browser closes for real.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![engine_send, protect, open_link, copy_secret, update_check, update_install, listen, open_browser, dapp_call])
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

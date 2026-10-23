//! What the Mac's app does around its window, the Windows way: the tray's panel (the menu-bar popover), notifications
//! by kind with a click that brings the window back, start at login (tauri-plugin-autostart, the registry's Run key),
//! one copy only (tauri-plugin-single-instance), the crash line on the next launch, and the update check.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{Emitter, Manager};

/// The app is quitting (Quit Completely, an update's restart): the engine's input closing then is no crash.
pub static QUITTING: AtomicBool = AtomicBool::new(false);
/// The engine's last state line, for the tray's panel when it opens.
static LAST_STATE: Mutex<String> = Mutex::new(String::new());
/// Where the tray icon was clicked (physical), so the panel stays anchored when it changes height; and when the
/// panel last hid on losing focus — a click on the icon then closes it instead of opening it again.
static ANCHOR: Mutex<Option<(f64, f64, f64, f64)>> = Mutex::new(None);
static HIDDEN_AT: Mutex<Option<Instant>> = Mutex::new(None);

pub const TRAY: &str = "tray";
const TRAY_WIDTH: f64 = 320.0;

// MARK: Engine lines

/// A state line: kept for the panel, and sent to it while it's open.
pub fn state_line(app: &tauri::AppHandle, line: &str) {
    if let Ok(mut last) = LAST_STATE.lock() { last.clear(); last.push_str(line); }
    if app.get_webview(TRAY).is_some() { let _ = app.emit_to(TRAY, "engine", line); }
}

/// A notification from the engine: its kind is already allowed there; one meant for a card the person may not see
/// (`whenAway`) stays quiet while the main window is in front.
pub fn notify(app: &tauri::AppHandle, v: &serde_json::Value) {
    use tauri_plugin_notification::NotificationExt;
    let data = &v["data"];
    if data["whenAway"].as_str() == Some("1") {
        if let Some(w) = app.get_webview_window("main") {
            if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) { return }
        }
    }
    let (title, body) = (data["title"].as_str().unwrap_or("ihatecrypto"), data["body"].as_str().unwrap_or(""));
    let _ = app.notification().builder().title(title).body(body).show();
}

/// A click on a notification (while it's on screen) brings the window back; one in the Action Center starts the app
/// again, and the single-instance callback does the same.
pub fn on_notification_click(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let handle = app.clone();
    let _ = app.notification().on_action(move |_| crate::show(&handle));
}

/// A line to the engine from the app itself (the crash line, the updater's news), as the page's engine_send.
fn tell_engine(app: &tauri::AppHandle, request: serde_json::Value) {
    use std::io::Write;
    if let Some(engine) = app.try_state::<crate::Engine>() {
        if let Ok(mut input) = engine.input.lock() { let _ = writeln!(input, "{request}").and_then(|_| input.flush()); }
    }
}

// MARK: Crashes

fn crash_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("Crashes"))
}

fn now_secs() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

/// A report for the next launch: «<seconds>-<who>.txt», its first line what the chat says.
pub fn write_crash(app: &tauri::AppHandle, who: &str, text: &str) {
    let Some(dir) = crash_dir(app) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{}-{who}.txt", now_secs())), text);
}

/// The window's own panic, kept before the process ends (release builds abort on panic).
pub fn keep_panics(app: &tauri::AppHandle) {
    let handle = app.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_crash(&handle, "window", &format!("the window stopped: {info}"));
        previous(info);
    }));
}

/// The newest report not said yet: one line in the chat, and the report is kept (renamed «.reported»).
pub fn report_crash(app: &tauri::AppHandle) {
    let Some(dir) = crash_dir(app) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut reports: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect();
    reports.sort();
    let Some(newest) = reports.pop() else { return };
    let text = std::fs::read_to_string(&newest).unwrap_or_default();
    let summary: String = text.lines().next().unwrap_or("").chars().take(200).collect();
    let secs = newest.file_stem().and_then(|s| s.to_str()).and_then(|s| s.split('-').next()).unwrap_or("0").to_string();
    tell_engine(app, serde_json::json!({ "op": "crashed", "name": secs, "text": summary, "address": dir.display().to_string() }));
    for path in reports.into_iter().chain([newest]) { let _ = std::fs::rename(&path, path.with_extension("reported")); }
}

// MARK: Updates

/// A newer version, checked a while after launch and then every six hours: the engine says it (its switch, its
/// language) and Settings › About installs it. Release builds only; a local build has no feed.
pub fn watch_updates(app: &tauri::AppHandle) {
    if cfg!(debug_assertions) { return }
    let handle = app.clone();
    std::thread::spawn(move || {
        use tauri_plugin_updater::UpdaterExt;
        let mut told = String::new();
        std::thread::sleep(Duration::from_secs(90));
        loop {
            let found = tauri::async_runtime::block_on(async {
                handle.updater().ok()?.check().await.ok().flatten().map(|u| u.version)
            });
            if let Some(version) = found.filter(|v| *v != told) {
                tell_engine(&handle, serde_json::json!({ "op": "updateReady", "text": version }));
                told = version;
            }
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

// MARK: The tray's panel

/// A left click on the tray icon: the panel above it (below it with the taskbar on top), or closed if it's open.
pub fn toggle_panel(app: &tauri::AppHandle, rect: tauri::Rect) {
    let scale = app.get_webview_window("main").and_then(|w| w.scale_factor().ok()).unwrap_or(1.0);
    let at = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    if let Ok(mut anchor) = ANCHOR.lock() { *anchor = Some((at.x, at.y, size.width, size.height)); }
    if let Some(panel) = app.get_webview_window(TRAY) {
        let just_hid = HIDDEN_AT.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_millis(400));
        if panel.is_visible().unwrap_or(false) || just_hid { let _ = panel.hide(); return }
        place(&panel, None);
        let _ = panel.show();
        let _ = panel.set_focus();
        let _ = panel.emit_to(TRAY, "opened", ());
        return;
    }
    let made = tauri::WebviewWindowBuilder::new(app, TRAY, tauri::WebviewUrl::App("tray.html".into()))
        .title("ihatecrypto").decorations(false).resizable(false).skip_taskbar(true).always_on_top(true).shadow(true)
        .visible(false).inner_size(TRAY_WIDTH, 360.0).build();
    if let Ok(panel) = made { place(&panel, None); let _ = panel.show(); let _ = panel.set_focus(); }
}

/// The panel's corner by the icon, inside the screen's work area.
fn place(panel: &tauri::WebviewWindow, height: Option<f64>) {
    let Some((x, y, w, h)) = ANCHOR.lock().ok().and_then(|a| *a) else { return };
    let scale = panel.scale_factor().unwrap_or(1.0);
    let size = panel.outer_size().map(|s| (s.width as f64, s.height as f64)).unwrap_or((TRAY_WIDTH * scale, 360.0 * scale));
    let (pw, ph) = (size.0, height.map(|h| h * scale).unwrap_or(size.1));
    // The icon's screen; the panel's own where the system can't say (macOS reads the point in other units).
    let Some(monitor) = panel.monitor_from_point(x + w / 2.0, y + h / 2.0).ok().flatten().or_else(|| panel.current_monitor().ok().flatten()) else { return };
    let area = monitor.work_area();
    let (left, top) = (area.position.x as f64, area.position.y as f64);
    let (right, bottom) = (left + area.size.width as f64, top + area.size.height as f64);
    let gap = 8.0 * scale;
    let px = (x + w / 2.0 - pw / 2.0).clamp(left + gap, (right - pw - gap).max(left + gap));
    // The taskbar at the bottom (the usual): above the icon; at the top: under it.
    let py = if y > top + (bottom - top) / 2.0 { (y - ph - gap).max(top + gap) } else { (y + h + gap).min(bottom - ph - gap) };
    let _ = panel.set_position(tauri::PhysicalPosition::new(px, py));
}

/// The panel hides when it loses focus, as the Mac's popover closes on a click elsewhere.
pub fn panel_blurred(window: &tauri::Window) {
    let _ = window.hide();
    if let Ok(mut t) = HIDDEN_AT.lock() { *t = Some(Instant::now()); }
}

fn from_panel(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == TRAY { Ok(()) } else { Err("only the tray's panel".into()) }
}

/// The panel's page measured itself: its height, kept by the icon.
#[tauri::command]
pub fn tray_layout(webview: tauri::Webview, height: f64) -> Result<(), String> {
    from_panel(&webview)?;
    let Some(panel) = webview.app_handle().get_webview_window(TRAY) else { return Ok(()) };
    let height = height.clamp(120.0, 640.0);
    panel.set_size(tauri::LogicalSize::new(TRAY_WIDTH, height)).map_err(|e| e.to_string())?;
    place(&panel, Some(height));
    Ok(())
}

/// What the panel draws when it opens: the engine's last state line.
#[tauri::command]
pub fn tray_state(webview: tauri::Webview) -> Result<String, String> {
    from_panel(&webview)?;
    Ok(LAST_STATE.lock().map(|s| s.clone()).unwrap_or_default())
}

/// «Open ihatecrypto» (and a click on a scheduled task) in the panel.
#[tauri::command]
pub fn tray_open(webview: tauri::Webview) -> Result<(), String> {
    from_panel(&webview)?;
    if let Some(panel) = webview.app_handle().get_webview_window(TRAY) { let _ = panel.hide(); }
    crate::show(webview.app_handle());
    Ok(())
}

/// «Quit Completely» in the panel.
#[tauri::command]
pub fn tray_quit(webview: tauri::Webview) -> Result<(), String> {
    from_panel(&webview)?;
    quit(webview.app_handle());
    Ok(())
}

pub fn quit(app: &tauri::AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
}

// MARK: The main window's commands

fn from_main(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "main" { Ok(()) } else { Err("only the main window".into()) }
}

/// Ctrl+Q: the window goes to the tray, the schedule, alerts and stops go on (the Mac's ⌘Q, Quit to Menu Bar).
#[tauri::command]
pub fn hide_main(webview: tauri::Webview) -> Result<(), String> {
    from_main(&webview)?;
    webview.window().hide().map_err(|e| e.to_string())
}

/// Ctrl+Shift+Q: quits for real (the Mac's ⌥⌘Q, Quit Completely).
#[tauri::command]
pub fn quit_app(webview: tauri::Webview) -> Result<(), String> {
    from_main(&webview)?;
    quit(webview.app_handle());
    Ok(())
}

/// Settings › Background › Open at login (the engine's `openAtLogin`): the registry's Run key, started with `--hidden` so
/// it waits in the tray. Set when the engine says so; how Windows has it goes back to the engine (`loginState`).
pub fn set_login(app: &tauri::AppHandle, v: &serde_json::Value) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let _ = if v["data"]["on"].as_str() == Some("1") { manager.enable() } else { manager.disable() };
    report_login(app);
}

/// At launch and after each change: the engine's switch shows what Windows has.
pub fn report_login(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    if let Ok(on) = app.autolaunch().is_enabled() { tell_engine(app, serde_json::json!({ "op": "loginState", "test": on })); }
}

/// Settings › Notifications › Windows settings: where Windows turns ihatecrypto's notifications on or off.
#[tauri::command]
pub fn open_notification_settings(webview: tauri::Webview) -> Result<(), String> {
    from_main(&webview)?;
    #[cfg(windows)]
    std::process::Command::new("explorer").arg("ms-settings:notifications").spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Started at login (`--hidden`): the window stays in the tray until it's opened.
pub fn started_hidden() -> bool { std::env::args().any(|a| a == "--hidden") }

/// The lock came down (the engine's `locked`): the dApp browser and the page beside the chat close, as the Mac closes
/// its browser and import windows on lock — nothing of the wallet stays on screen behind the lock.
pub fn locked(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window(crate::BROWSER_WINDOW) { let _ = window.close(); }
    for label in [crate::web::PANEL, crate::web::PANEL_BAR] { if let Some(v) = app.get_webview(label) { let _ = v.close(); } }
}

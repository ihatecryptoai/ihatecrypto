//! The window: the page in ../ui over the engine. The engine (ihatecrypto-engine, Swift) runs as this process's child
//! with pipes; every line it writes reaches the page as the «engine» event, every line the page sends goes to it. The
//! engine starts the signer itself, so this process never holds a key or talks to the signer.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;

use tauri::{Emitter, EventTarget, Manager};

struct Engine {
    input: Mutex<ChildStdin>,
    child: Mutex<Child>,
    /// The browser's calls waiting for the engine's answer, by request id (from DAPP_IDS up, so the page's own never meet them).
    dapp: Mutex<HashMap<i64, mpsc::Sender<String>>>,
    next: AtomicI64,
    browser: Mutex<BrowserPage>,
    /// The page beside the canvas (web.rs), and where its panel sits.
    side: Mutex<BrowserPage>,
    panel: Mutex<web::PanelPlace>,
}

const DAPP_IDS: i64 = 1_000_000_000;

/// What the browser's toolbar shows beside the page's address: the page's title and whether it loads, the toolbar's
/// height (it grows for the phishing banner), and from the engine's last state the active wallet and the language.
#[derive(Default)]
struct BrowserPage {
    title: String,
    loading: bool,
    bar: f64,
    wallet: Option<String>,
    russian: bool,
    /// The address last opened: what the toolbar shows while WebView2 still says about:blank (loading, or a site that
    /// doesn't answer).
    asked: Option<tauri::Url>,
    /// WebView2's own word on the history, a page's pushState included (web.rs); off Windows both stay on, as the Mac's
    /// Back is.
    back: bool,
    forward: bool,
    /// Why the last page didn't load («timeout», «name», «connect», «certificate», «crashed», «other:N»).
    error: Option<String>,
    /// A host on MetaMask's phishing list the browser didn't open, and the hosts it has refused this run.
    blocked: Option<String>,
    refused: HashSet<String>,
    /// Beside the chat: whether it shows now (the dApp browser hidden behind a page, the page panel closed). A browser
    /// behind a page keeps its page alive, as the Mac's.
    shown: bool,
    /// The site's origin as its page last started: a waiting request of that origin goes when its page does (siteGone).
    origin: Option<String>,
}

/// The browser: one window, our toolbar page on top (webview «browser-bar») and the site under it (webview «browser»).
const BROWSER_WINDOW: &str = "browser-window";
const BAR: &str = "browser-bar";
const SITE: &str = "browser";
const BAR_HEIGHT: f64 = 52.0;
/// Where the toolbar starts: on Windows the client area's top. macOS (a developer's run of this window) lays child
/// webviews out from the window's own top, under its 32-point title bar.
const TOP: f64 = if cfg!(target_os = "macos") { 32.0 } else { 0.0 };

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
            // The toolbar shows the active wallet and, after a Confirm on the canvas, that the site is connected: each
            // state tells it to look again. Anywhere in the line: the engine's encoder on Windows needn't put «event» first.
            if line.contains("\"event\":\"state\"") {
                system::state_line(&handle, &line);
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(engine) = handle.try_state::<Engine>() {
                        if let Ok(mut page) = engine.browser.lock() {
                            let a = &v["data"]["addresses"];
                            // Watched addresses have no key: the bar says a site can't get a signature here.
                            page.wallet = if v["data"]["watching"].as_bool() == Some(true) { None }
                                else { a["solana"].as_str().or(a["evm"].as_str()).map(short) };
                            page.russian = v["data"]["russian"].as_bool() == Some(true);
                        }
                        if let Ok(mut side) = engine.side.lock() { side.russian = v["data"]["russian"].as_bool() == Some(true); }
                    }
                }
                if handle.get_webview(BAR).is_some() { let _ = handle.emit_to(EventTarget::webview(BAR), "browser", ()); }
            }
            // «open jup.ag» in the chat: the site in the browser, as the Mac's chat opens it.
            if line.contains("\"event\":\"openSite\"") {
                if let Some(url) = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|v| v["data"]["url"].as_str().map(String::from)) {
                    tauri::async_runtime::spawn(open_browser(handle.clone(), url));
                }
            }
            // The lock: the browser and the page panel close behind it.
            if line.contains("\"event\":\"locked\"") { system::locked(&handle); }
            // Settings or the onboarding turned Open at login on or off.
            if line.contains("\"event\":\"openAtLogin\"") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) { system::set_login(&handle, &v); }
            }
            // Settings turned Keep always running on or off.
            if line.contains("\"event\":\"keepRunning\"") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) { system::set_keep_running(&handle, &v); }
            }
            // A site's request is on the canvas: the main window comes forward to show it.
            if line.contains("\"event\":\"showMain\"") { show(&handle); }
            // The signer is about to ask Windows Hello: only the window in front may let it come forward.
            if line.contains("\"event\":\"signerForeground\"") {
                if let Some(pid) = serde_json::from_str::<serde_json::Value>(&line).ok()
                    .and_then(|v| v["data"]["pid"].as_str().and_then(|p| p.parse::<u32>().ok())) {
                    allow_foreground(pid);
                    bring_hello_forward();
                }
            }
            // Money that arrived, a price alert, a scheduled move waiting for Confirm: a Windows notification too,
            // since the window may be in the tray.
            if line.contains("\"event\":\"notify\"") {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) { system::notify(&handle, &v); }
            }
            // Only to the main window: a page in the browser never hears the engine.
            let _ = handle.emit_to("main", "engine", line);
        }
        let _ = handle.emit_to("main", "engine", "{\"event\":\"engineStopped\",\"data\":{}}");
        // Not on the way out: kept for the chat's line at the next launch.
        if !system::QUITTING.load(Ordering::SeqCst) {
            let code = handle.try_state::<Engine>().and_then(|e| e.child.lock().ok().and_then(|mut c| c.try_wait().ok().flatten()))
                .and_then(|status| status.code()).map(|c| format!(" (exit code {c:#x})")).unwrap_or_default();
            system::write_crash(&handle, "engine", &format!("the engine stopped{code}"));
        }
    });
    Ok(Engine { input: Mutex::new(input), child: Mutex::new(child), dapp: Mutex::new(HashMap::new()), next: AtomicI64::new(DAPP_IDS),
                browser: Mutex::new(BrowserPage { bar: BAR_HEIGHT, ..Default::default() }), side: Mutex::new(BrowserPage::default()),
                panel: Mutex::new(web::PanelPlace::default()) })
}

/// «HAgk…Kpqk».
fn short(address: &str) -> String {
    let chars: Vec<char> = address.chars().collect();
    if chars.len() <= 10 { return address.into() }
    format!("{}…{}", chars[..4].iter().collect::<String>(), chars[chars.len() - 4..].iter().collect::<String>())
}

/// A request of the app's own to the engine (the browser's, never a page's line), answered by its id.
async fn engine_call(state: &Engine, request: serde_json::Value, wait: std::time::Duration) -> Result<serde_json::Value, String> {
    let (id, rx) = engine_ask(state, request)?;
    let answer = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(wait)).await.map_err(|e| e.to_string())?;
    engine_answer(state, id, answer.ok())
}

/// The same, waiting on this thread: only for a short wait (the browser asks before a page starts).
fn engine_call_now(state: &Engine, request: serde_json::Value, wait: std::time::Duration) -> Result<serde_json::Value, String> {
    let (id, rx) = engine_ask(state, request)?;
    engine_answer(state, id, rx.recv_timeout(wait).ok())
}

fn engine_ask(state: &Engine, mut request: serde_json::Value) -> Result<(i64, mpsc::Receiver<String>), String> {
    let id = state.next.fetch_add(1, Ordering::SeqCst);
    request["id"] = id.into();
    let (tx, rx) = mpsc::channel();
    state.dapp.lock().map_err(|_| "the engine is gone")?.insert(id, tx);
    let mut input = state.input.lock().map_err(|_| "the engine is gone")?;
    writeln!(input, "{request}").and_then(|_| input.flush()).map_err(|e| format!("the engine stopped: {e}"))?;
    Ok((id, rx))
}

fn engine_answer(state: &Engine, id: i64, answer: Option<String>) -> Result<serde_json::Value, String> {
    let Some(answer) = answer else {
        if let Ok(mut waiting) = state.dapp.lock() { waiting.remove(&id); }
        return Err("No answer: the request timed out.".into())
    };
    serde_json::from_str(&answer).map_err(|e| e.to_string())
}

/// The site's page is gone — closed, the app locked, or another origin loaded in its webview: a request of that origin
/// still waiting goes with it (the engine declines it), as the Mac's sheet goes with its browser view. No answer awaited.
fn site_gone(app: &tauri::AppHandle, origin: Option<String>) {
    let (Some(origin), Some(engine)) = (origin, app.try_state::<Engine>()) else { return };
    let _ = engine_ask(&engine, serde_json::json!({ "op": "siteGone", "address": origin }));
}

/// The same for the page the site has now, which is closing.
fn site_closed(app: &tauri::AppHandle) {
    let origin = app.try_state::<Engine>().and_then(|e| e.browser.lock().ok().and_then(|mut p| p.origin.take()));
    site_gone(app, origin);
}

/// Whether the browser may go to `url`: https (or a blank page), and not a host on MetaMask's phishing list, which never
/// loads, as on the Mac. The engine has the list; asked here before the page starts, briefly, and a slow answer lets it
/// start: the check when it finishes loading turns it back.
fn site_allowed(app: &tauri::AppHandle, url: &tauri::Url) -> bool {
    if url.as_str() == "about:blank" { return true }
    let (Some(origin), Some(host)) = (origin_of(url), url.host_str()) else { return false };
    let Some(engine) = app.try_state::<Engine>() else { return true };
    let known = engine.browser.lock().map(|p| p.refused.contains(host)).unwrap_or(false);
    let listed = known || engine_call_now(&engine, serde_json::json!({ "op": "siteState", "address": origin }), std::time::Duration::from_millis(400))
        .map(|v| v["result"]["blocked"] == "true" || v["result"]["blocked"] == true).unwrap_or(false);
    if let Ok(mut page) = engine.browser.lock() {
        if listed {
            page.blocked = Some(host.to_string());
            page.refused.insert(host.to_string());
            page.asked = Some(url.clone());
        } else {
            page.blocked = None;
        }
    }
    if listed { let _ = app.emit_to(EventTarget::webview(BAR), "browser", ()); }
    !listed
}

/// The origin a page's calls are for, from its webview's own address: https only.
fn origin_of(url: &tauri::Url) -> Option<String> {
    let (Some(host), "https") = (url.host_str(), url.scheme()) else { return None };
    Some(format!("https://{host}{}", url.port().map(|p| format!(":{p}")).unwrap_or_default()))
}

/// What the person typed or the canvas asked for, as a site's address: a bare «jup.ag» is https; nothing but https opens.
fn site_url(text: &str) -> Result<tauri::Url, String> {
    let text = text.trim();
    let full = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    let url = tauri::Url::parse(&full).map_err(|_| format!("Not a site address: {text}"))?;
    if url.scheme() != "https" || url.host_str().is_none() { return Err("ihatecrypto's browser opens https sites only.".into()) }
    Ok(url)
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

mod system;
mod voice;
mod web;

/// The mic: off the main thread, since it waits until the person stops speaking.
#[tauri::command]
async fn listen() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(voice::listen).await.map_err(|e| e.to_string())?
}

/// The chat's mic with the words as they're heard (the Mac's live transcript): each partial text goes to the main page
/// as a «voice» event; the answer is the final text. Only the main window's own page may listen.
#[tauri::command]
async fn listen_live(webview: tauri::Webview) -> Result<String, String> {
    if webview.label() != "main" { return Err("only the main window".into()) }
    let app = webview.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || voice::listen_live(move |text| { let _ = app.emit_to(EventTarget::webview("main"), "voice", text); }))
        .await.map_err(|e| e.to_string())?
}

/// Cancel (Esc) or Done (Return) on the live listening.
#[tauri::command]
fn listen_stop(webview: tauri::Webview, done: bool) -> Result<(), String> {
    if webview.label() != "main" { return Err("only the main window".into()) }
    voice::stop(done);
    Ok(())
}

#[tauri::command]
fn engine_send(state: tauri::State<'_, Engine>, line: String) -> Result<(), String> {
    if line.contains('\n') { return Err("one line at a time".into()) }
    let mut input = state.input.lock().map_err(|_| "the engine is gone")?;
    writeln!(input, "{line}").and_then(|_| input.flush()).map_err(|e| format!("the engine stopped: {e}"))
}

/// ihatecrypto's browser, as the Mac's: our toolbar on top (back, reload, the address and its check, the connection)
/// and the site under it with the wallet provider in every page, in the panel beside the chat («open jup.ag», the
/// profile menu's Browser, a portfolio row, a page's «with the wallet») and, by its «Move to its own window», in a
/// window of its own. The site shares nothing with either of our pages; its only way to the wallet is `dapp_call`.
// Async: on Windows a webview made inside a synchronous command waits on the event loop that command holds, and the
// window stays white (wry #583).
#[tauri::command]
async fn open_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = site_url(&url)?;
    if let Some(engine) = app.try_state::<Engine>() {
        if let Ok(mut page) = engine.browser.lock() { page.asked = Some(url.clone()); }
    }
    // One panel at a time, as the Mac's: a page beside the chat gives way to the site.
    web::close_page(&app);
    let main = app.get_window("main").ok_or("The main window is closed.")?;
    if !main.is_visible().unwrap_or(true) { show(&app); }
    if let Some(site) = app.get_webview(SITE) {
        // The same page again only comes back; another one opens in it.
        if site.url().map(|u| u != url).unwrap_or(true) { site.navigate(url).map_err(|e| e.to_string())?; }
        // In its own window: back beside the chat with its page and connection, as the Mac's openSite closes that
        // window first — one web view can't show in two places.
        if site.window().label() != "main" {
            let own = site.window();
            for label in [BAR, SITE] { if let Some(v) = app.get_webview(label) { v.reparent(&main).map_err(|e| e.to_string())?; } }
            let _ = own.close();
            let _ = app.emit_to(EventTarget::webview(BAR), "browser", ());
        }
        // Put away behind a page, or back from its own window: it shows beside the chat again.
        web::shown(&app, SITE, true);
        web::ensure_edge(&app)?;
        web::layout_panel(&app);
        return Ok(());
    }
    // A new browser starts clean: the last one's title and banner height aren't this page's.
    if let Some(engine) = app.try_state::<Engine>() {
        if let Ok(mut page) = engine.browser.lock() { page.title.clear(); page.loading = true; page.bar = BAR_HEIGHT + web::PANEL_HEAD; }
    }
    main.add_child(tauri::webview::WebviewBuilder::new(BAR, tauri::WebviewUrl::App("browser-bar.html".into())),
                   tauri::LogicalPosition::new(0.0, 0.0), tauri::LogicalSize::new(1.0, 1.0)).map_err(|e| e.to_string())?;
    let site = main.add_child(site_view(&app, url), tauri::LogicalPosition::new(0.0, 0.0), tauri::LogicalSize::new(1.0, 1.0))
        .map_err(|e| e.to_string())?;
    web::watch(&site, &app);
    web::shown(&app, SITE, true);
    web::ensure_edge(&app)?;
    web::layout_panel(&app);
    Ok(())
}

/// The browser's own window, which «Move to its own window» moves the toolbar and the site into, page and connection kept.
fn browser_window(app: &tauri::AppHandle) -> Result<tauri::Window, String> {
    let window = tauri::window::Window::builder(app, BROWSER_WINDOW)
        .title("ihatecrypto · browser")
        .inner_size(1180.0, 800.0)
        .min_inner_size(640.0, 420.0)
        .build()
        .map_err(|e| e.to_string())?;
    if !has_room(&window, 1180.0, 800.0) { let _ = window.maximize(); }
    for label in [BAR, SITE] { if let Some(v) = app.get_webview(label) { v.reparent(&window).map_err(|e| e.to_string())?; } }
    layout_browser(&window);
    // Nothing beside the chat now: its edge goes out of sight.
    web::layout_panel(app);
    let _ = window.set_focus();
    let _ = app.emit_to(EventTarget::webview(BAR), "browser", ());
    Ok(window)
}

/// The site's webview, wherever it shows: https only, the wallet provider in its page (never a frame's), the phishing
/// check before and after each load.
fn site_view(app: &tauri::AppHandle, url: tauri::Url) -> tauri::webview::WebviewBuilder<tauri::Wry> {
    let (loads, titles, goes) = (app.clone(), app.clone(), app.clone());
    tauri::webview::WebviewBuilder::new(SITE, tauri::WebviewUrl::External(url))
        .initialization_script(include_str!("provider.js"))
        // Pages stay on https; anything else (a file, a custom scheme) isn't opened here, nor a phishing site.
        .on_navigation(move |u| site_allowed(&goes, u))
        // A link for a new window: an https page goes to the person's own browser; the browser stays one page, as the
        // Mac's, which opens no second window either.
        .on_new_window(|u, _| {
            if u.scheme() == "https" { let _ = open_link(u.to_string()); }
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |site, payload| {
            let started = matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
            let mut gone = None;
            if let Some(engine) = loads.try_state::<Engine>() {
                if let Ok(mut page) = engine.browser.lock() {
                    page.loading = started;
                    if started {
                        page.error = None;
                        // Another origin's page: the one that asked is gone, and so is its waiting request.
                        // ponytail: a reload of the same origin keeps it, as the contract with the engine says (siteGone).
                        let now = origin_of(payload.url());
                        if page.origin.is_some() && page.origin != now { gone = page.origin.clone(); }
                        page.origin = now;
                    }
                }
            }
            site_gone(&loads, gone);
            // A page the quick check let through while the engine was slow: looked at again once it's loaded.
            if !started {
                let (app, url) = (loads.clone(), payload.url().clone());
                tauri::async_runtime::spawn(async move {
                    if !site_allowed_late(&app, &url).await { let _ = site.navigate("about:blank".parse().expect("blank")); }
                });
            }
            let _ = loads.emit_to(EventTarget::webview(BAR), "browser", ());
        })
        .on_document_title_changed(move |_, title| {
            if let Some(engine) = titles.try_state::<Engine>() {
                if let Ok(mut page) = engine.browser.lock() { page.title = title; }
            }
            let _ = titles.emit_to(EventTarget::webview(BAR), "browser", ());
        })
}

/// The check after a page loaded, with all the time the engine needs.
async fn site_allowed_late(app: &tauri::AppHandle, url: &tauri::Url) -> bool {
    let (Some(origin), Some(host)) = (origin_of(url), url.host_str()) else { return true };
    let Some(engine) = app.try_state::<Engine>() else { return true };
    let listed = engine_call(&engine, serde_json::json!({ "op": "siteState", "address": origin }), std::time::Duration::from_secs(20)).await
        .map(|v| v["result"]["blocked"] == "true" || v["result"]["blocked"] == true).unwrap_or(false);
    if listed {
        if let Ok(mut page) = engine.browser.lock() {
            page.blocked = Some(host.to_string());
            page.refused.insert(host.to_string());
            page.asked = Some(url.clone());
        }
        let _ = app.emit_to(EventTarget::webview(BAR), "browser", ());
    }
    !listed
}

/// Whether the window's screen fits it with the taskbar and some margin; a small laptop or a VM opens it maximized.
fn has_room(window: &tauri::Window, width: f64, height: f64) -> bool {
    let Ok(Some(monitor)) = window.current_monitor() else { return true };
    let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
    size.width >= width + 80.0 && size.height >= height + 80.0
}

/// The toolbar across the top at its own height, the site in the rest: on every resize and change of scale.
fn layout_browser(window: &tauri::Window) {
    // macOS (a developer's run) gives a window whose webviews came by «Move to its own window» its first webview's size
    // as its own (tauri-runtime-wry's inner_size); its frame is the window's. Windows reads the window itself.
    let size = if cfg!(target_os = "macos") { window.outer_size() } else { window.inner_size() };
    let (Ok(size), Ok(scale)) = (size, window.scale_factor()) else { return };
    let size = size.to_logical::<f64>(scale);
    let app = window.app_handle();
    let bar = app.try_state::<Engine>().and_then(|e| e.browser.lock().ok().map(|p| p.bar)).unwrap_or(BAR_HEIGHT);
    let rect = |y: f64, height: f64| tauri::Rect { position: tauri::LogicalPosition::new(0.0, y).into(), size: tauri::LogicalSize::new(size.width, height.max(0.0)).into() };
    if let Some(v) = app.get_webview(BAR) { let _ = v.set_bounds(rect(TOP, bar)); }
    if let Some(v) = app.get_webview(SITE) { let _ = v.set_bounds(rect(TOP + bar, size.height - TOP - bar)); }
}

/// The toolbar's buttons: back, forward, reload, stop, open an address, and Disconnect (the engine forgets the site,
/// and the page reloads so it forgets the address too). Only the toolbar's own page may ask.
#[tauri::command]
async fn browser_nav(webview: tauri::Webview, state: tauri::State<'_, Engine>, action: String, url: Option<String>) -> Result<(), String> {
    if webview.label() != BAR { return Err("only the browser's toolbar".into()) }
    let site = webview.app_handle().get_webview(SITE).ok_or("No page is open.")?;
    let done = match action.as_str() {
        "back" => web::go(&site, true),
        "forward" => web::go(&site, false),
        "reload" => site.reload(),
        "stop" => site.eval("window.stop()"),
        "open" => {
            let url = site_url(url.as_deref().unwrap_or(""))?;
            if let Ok(mut page) = state.browser.lock() { page.asked = Some(url.clone()); }
            site.navigate(url)
        }
        "disconnect" => {
            let origin = origin_of(&site.url().map_err(|e| e.to_string())?).ok_or("Not an https site.")?;
            let line = serde_json::json!({ "op": "dapp", "name": "disconnect", "text": "{\"everywhere\":true}", "address": origin });
            engine_call(&state, line, std::time::Duration::from_secs(10)).await?;
            site.reload()
        }
        // The panel's «Move to its own window»: the same page, its connection kept.
        "window" if web::wallet_in_panel(webview.app_handle()) => { browser_window(webview.app_handle())?; Ok(()) }
        // The panel's close; in its own window the window's close does it.
        "close" => {
            let app = webview.app_handle();
            if web::wallet_in_panel(app) {
                site_closed(app);
                web::shown(app, SITE, false);
                for label in [SITE, BAR] { if let Some(v) = app.get_webview(label) { let _ = v.close(); } }
                web::layout_panel(app);
            }
            else if let Some(w) = app.get_window(BROWSER_WINDOW) { let _ = w.close(); }
            Ok(())
        }
        _ => return Err(format!("unknown action {action}")),
    };
    done.map_err(|e| e.to_string())
}

/// What the toolbar draws: the page's address and title, whether it loads, and the engine's word on the site — known,
/// a lookalike of which, or unknown; connected or not — with the wallet the site would get.
#[tauri::command]
async fn browser_state(webview: tauri::Webview, state: tauri::State<'_, Engine>) -> Result<serde_json::Value, String> {
    if webview.label() != BAR { return Err("only the browser's toolbar".into()) }
    let site = webview.app_handle().get_webview(SITE).ok_or("No page is open.")?;
    let mut url = site.url().map_err(|e| e.to_string())?;
    let mut out = {
        let page = state.browser.lock().map_err(|_| "the engine is gone")?;
        // While WebView2 says about:blank, and after a refused phishing page (the last one stays, the field shows what was asked).
        if url.scheme() != "https" || page.blocked.is_some() { if let Some(asked) = &page.asked { url = asked.clone(); } }
        serde_json::json!({ "url": url.as_str(), "title": page.title, "loading": page.loading, "wallet": page.wallet, "russian": page.russian,
                            "connected": false, "check": "local", "real": null, "blocked": page.blocked.is_some(), "blockedHost": page.blocked,
                            "back": page.back || !web::NATIVE_HISTORY, "forward": page.forward || !web::NATIVE_HISTORY, "error": page.error,
                            "panel": web::wallet_in_panel(webview.app_handle()) })
    };
    if let Some(origin) = origin_of(&url) {
        out["check"] = "unknown".into();
        // An engine without the request yet, or slow to answer: the site stays «unknown», not connected.
        let line = serde_json::json!({ "op": "siteState", "address": origin });
        if let Ok(v) = engine_call(&state, line, std::time::Duration::from_secs(3)).await {
            if v["ok"].as_bool() == Some(true) {
                // The engine answers in strings («true»); only a plain yes counts.
                let r = &v["result"];
                out["connected"] = (r["connected"] == true || r["connected"] == "true").into();
                if r["blocked"] == true || r["blocked"] == "true" { out["blocked"] = true.into(); }
                if let Some(check) = r["check"].as_str() { out["check"] = check.into(); }
                if let Some(real) = r["real"].as_str() { out["real"] = real.into(); }
            }
        }
    }
    Ok(out)
}

/// The toolbar's height, as its page measured it: taller while the phishing banner shows.
#[tauri::command]
fn browser_layout(webview: tauri::Webview, state: tauri::State<'_, Engine>, height: f64) -> Result<(), String> {
    if webview.label() != BAR { return Err("only the browser's toolbar".into()) }
    state.browser.lock().map_err(|_| "the engine is gone")?.bar = height.clamp(40.0, 260.0);
    if web::wallet_in_panel(webview.app_handle()) { web::layout_panel(webview.app_handle()) } else { layout_browser(&webview.window()) }
    Ok(())
}

/// A page's call to the wallet. The origin is the browser window's own address, read here, never what the page says;
/// the engine puts the request on the canvas and answers once the person pressed Confirm or Cancel.
#[tauri::command]
async fn dapp_call(webview: tauri::Webview, request: tauri::ipc::Request<'_>, state: tauri::State<'_, Engine>, method: String,
                   params: serde_json::Value) -> Result<String, String> {
    if webview.label() != SITE { return Err("only the browser's pages".into()) }
    let origin = origin_of(&webview.url().map_err(|e| e.to_string())?).ok_or("only https sites get the wallet")?;
    // From the page itself, never a frame inside it: a frame on another origin (an ad, a widget) gets the same IPC, and
    // its ask would show under the page's name. Tauri puts the sender's Origin on the request; the Mac checks the main
    // frame (Browser.swift). With no Origin the call came by postMessage, which WebView2 gives no frame. A frame on the
    // page's own origin passes: it can reach the page anyway. Checked on Windows 11: a sandboxed frame is refused.
    if let Some(sender) = request.headers().get("Origin").and_then(|v| v.to_str().ok()) {
        if sender.trim_end_matches('/') != origin { return Err("only the page itself, not a frame inside it".into()) }
    }
    const METHODS: [&str; 6] = ["connect", "disconnect", "signMessage", "signTransaction", "signAndSendTransaction", "eth"];
    if !METHODS.contains(&method.as_str()) { return Err(format!("ihatecrypto doesn't support {method}.")) }
    // The page's title for the second check (Jev reads host and title, as on the Mac); the toolbar's own copy of it.
    let title = state.browser.lock().map(|p| p.title.clone()).unwrap_or_default();
    let line = serde_json::json!({ "op": "dapp", "name": method, "text": params.to_string(), "address": origin, "label": title });
    // The person may take a while to read the card.
    let v = engine_call(&state, line, std::time::Duration::from_secs(900)).await?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["result"]["json"].as_str().unwrap_or("null").to_string())
    } else {
        Err(v["error"].as_str().unwrap_or("Declined.").to_string())
    }
}

/// `http://127.0.0.1:<port>/<32 lowercase hex>` and nothing else: the Top up page the engine serves on loopback for a
/// browser wallet (TopUp.swift), under a one-time token. Safe to hand to the person's browser: this machine only, no
/// other host, no query, no path beyond the token — a shell or the browser can read nothing more into it.
fn loopback_top_up(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else { return false };
    let Some((port, token)) = rest.split_once('/') else { return false };
    let port_ok = !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) && !port.starts_with('0')
        && port.parse::<u32>().is_ok_and(|p| p <= 65535);
    port_ok && token.len() == 32 && token.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::loopback_top_up;

    #[test]
    fn only_the_top_up_page_on_loopback_passes() {
        let token = "0123456789abcdef0123456789abcdef";
        assert!(loopback_top_up(&format!("http://127.0.0.1:51123/{token}")));
        for bad in [
            format!("http://127.0.0.1:51123/{token}/"), format!("http://127.0.0.1:51123/{token}?x=1"), format!("http://127.0.0.1:51123/{token}0"),
            format!("http://127.0.0.1:51123/{}", token.to_uppercase()), format!("http://localhost:51123/{token}"), format!("http://127.0.0.2:51123/{token}"),
            format!("http://127.0.0.1/{token}"), format!("http://127.0.0.1:0/{token}"), format!("http://127.0.0.1:70000/{token}"),
            format!("http://127.0.0.1:51123@evil.com/{token}"), format!("https://127.0.0.1:51123/{token}"), format!("http://127.0.0.1:51123/../{token}"),
        ] {
            assert!(!loopback_top_up(&bad), "{bad}");
        }
    }
}

/// While the recovery phrase is on screen the window is left out of screenshots and screen sharing.
#[tauri::command]
fn protect(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_content_protected(on).map_err(|e| e.to_string())
}

/// An explorer link in the default browser: https only, nothing that a shell could read as more than a link — and the
/// engine's one-time Top up page for a browser wallet (`loopback_top_up`).
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    let plain = url.starts_with("https://") && url.len() < 512 && url.chars().all(|c| c.is_ascii_graphic() && !"\"'`<>|^%&".contains(c));
    if !plain && !loopback_top_up(&url) { return Err("not a plain https link".into()) }
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

/// The words off the clipboard now («I wrote them down», leaving the screen), as the Mac wipes them: only if the
/// clipboard still holds what copy_secret put there.
#[tauri::command]
fn clear_secret() {
    #[cfg(windows)]
    secret_clipboard::clear_now();
}

#[cfg(windows)]
mod secret_clipboard {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    use windows::core::w;

    const CF_UNICODETEXT: u32 = 13;
    /// The clipboard's sequence number right after the words went on it; 0 once they're gone.
    static COPIED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    pub fn clear_now() {
        use std::sync::atomic::Ordering;
        let copied = COPIED.swap(0, Ordering::SeqCst);
        unsafe {
            if copied != 0 && GetClipboardSequenceNumber() == copied && OpenClipboard(None).is_ok() {
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
            }
        }
    }

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
            COPIED.store(copied, std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(60));
                // Only these words: something copied since, or a wipe already done, is left alone.
                if COPIED.load(std::sync::atomic::Ordering::SeqCst) == copied { clear_now() }
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
                // «browser:» lines go to the site's page instead, «bar:» to the browser's toolbar, «panel:» and
                // «panelbar:» to the page beside the canvas and its header, «edge:» to its edge; «show:<label>» brings that window forward.
                // «tray:x,y» opens the tray's panel as a click on an icon at that point would (physical pixels).
                if let Some(at) = line.strip_prefix("tray:") {
                    let xy: Vec<f64> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                    let rect = tauri::Rect { position: tauri::PhysicalPosition::new(*xy.first().unwrap_or(&1000.0), *xy.get(1).unwrap_or(&0.0)).into(),
                                             size: tauri::PhysicalSize::new(44.0, 44.0).into() };
                    let app = handle.clone();
                    let _ = handle.run_on_main_thread(move || system::toggle_panel(&app, rect));
                    done += 1;
                    continue;
                }
                if let Some(label) = line.strip_prefix("show:") {
                    if let Some(window) = handle.get_window(label) { let _ = window.show(); let _ = window.set_focus(); }
                    done += 1;
                    continue;
                }
                let (label, js) = line.strip_prefix("browser:").map(|js| (SITE, js))
                    .or_else(|| line.strip_prefix("bar:").map(|js| (BAR, js)))
                    .or_else(|| line.strip_prefix("panel:").map(|js| (web::PANEL, js)))
                    .or_else(|| line.strip_prefix("panelbar:").map(|js| (web::PANEL_BAR, js)))
                    .or_else(|| line.strip_prefix("edge:").map(|js| (web::PANEL_EDGE, js))).unwrap_or(("main", line));
                if let Some(webview) = handle.get_webview(label) { let _ = webview.eval(js); }
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
    // Downloaded and checked first; the task rests only once the installer really runs, or it could start the old copy
    // mid-swap — and a download or install that fails leaves it going (the Mac's updaterWillRelaunchApplication).
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    system::pause_keep_running();
    if let Err(e) = update.install(bytes) {
        system::resume_keep_running();
        return Err(e.to_string());
    }
    system::QUITTING.store(true, Ordering::SeqCst);
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
    let quit = MenuItem::with_id(app, "quit", "Quit Completely", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::new().tooltip("ihatecrypto").menu(&menu).show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show(app),
            "quit" => system::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event { system::toggle_panel(tray.app_handle(), rect) }
        });
    if let Some(icon) = app.default_window_icon() { builder = builder.icon(icon.clone()); }
    builder.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // One copy only, as on the Mac: a second one would run the schedule and the stops twice. Starting it again (a
        // notification in the Action Center, the Start menu) brings this one's window back.
        // A second start shows the first copy's window, unless it's Keep always running's minutely start (`--hidden`).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| if !args.iter().any(|a| a == "--hidden") { show(app) }))
        // Settings › Background › Open at login: the registry's Run key, starting in the tray.
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        // Updates come signed with the key in tauri.conf.json (plugins.updater.pubkey); anything else is refused.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            system::keep_panics(app.handle());
            let engine = start_engine(app.handle())?;
            app.manage(engine);
            system::on_notification_click(app.handle());
            system::report_crash(app.handle());
            system::report_login(app.handle());
            // Quit Completely rested Keep running's task; opened again, it runs again (the Mac's KeepRunning.resume).
            system::resume_keep_running();
            system::report_keep_running(app.handle());
            system::watch_updates(app.handle());
            #[cfg(debug_assertions)]
            ui_eval(app.handle());
            tray(app)?;
            // The Mac's size (1320×860). A screen without room for it (a small laptop, a VM): it opens maximized instead of reaching under
            // the taskbar.
            if let Some(window) = app.get_webview_window("main") {
                if !has_room(&window.as_ref().window(), 1320.0, 860.0) { let _ = window.maximize(); }
                // Started at login it waits in the tray, its window closed (tauri.conf keeps it hidden until here).
                if !system::started_hidden() { let _ = window.show(); let _ = window.set_focus(); }
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(focused) if window.label() == "main" => system::window_focus(window.app_handle(), *focused),
            // The main window closes to the tray; the browser closes for real.
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Focused(false) if window.label() == system::TRAY => system::panel_blurred(window),
            // The browser's own window closed with its site (not when the site went back beside the chat first).
            tauri::WindowEvent::Destroyed if window.label() == BROWSER_WINDOW && !web::wallet_in_panel(window.app_handle()) => site_closed(window.app_handle()),
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } if window.label() == BROWSER_WINDOW => layout_browser(window),
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } if window.label() == "main" => web::layout_panel(window.app_handle()),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![engine_send, protect, open_link, copy_secret, clear_secret, update_check, update_install, listen, open_browser, dapp_call,
            browser_nav, browser_state, browser_layout, web::open_page, web::page_nav, web::page_state, web::page_layout,
            system::tray_layout, system::tray_state, system::tray_open, system::tray_quit, system::hide_main, system::quit_app,
            system::open_notification_settings, listen_live, listen_stop])
        .build(tauri::generate_context!())
        .expect("ihatecrypto's window didn't start")
        .run(|app, event| {
            // The engine ends when its input closes; it is stopped outright when the app quits.
            if let tauri::RunEvent::Exit = event {
                system::QUITTING.store(true, Ordering::SeqCst);
                if let Some(engine) = app.try_state::<Engine>() {
                    if let Ok(mut child) = engine.child.lock() { let _ = child.kill(); }
                }
            }
        });
}

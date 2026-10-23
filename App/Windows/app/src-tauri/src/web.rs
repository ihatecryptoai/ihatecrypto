//! What our webviews of web pages learn from WebView2 itself (Back and Forward as it has them, a page's pushState too;
//! why a page didn't load; a page's process that ended), and the page beside the canvas: the Mac's WebPanel.

use tauri::{Emitter, EventTarget, Manager};

use crate::{BrowserPage, Engine, BAR, SITE};

/// Windows reads Back and Forward from WebView2. Elsewhere (a developer's run on a Mac) both stay on, as the Mac's Back is.
pub const NATIVE_HISTORY: bool = cfg!(windows);

/// The page beside the canvas: our header (back, reload, the page's name, copy, with the wallet, the person's browser,
/// close) over a plain web view.
pub const PANEL_BAR: &str = "panel-bar";
pub const PANEL: &str = "panel";
const PANEL_BAR_HEIGHT: f64 = 46.0;

/// Where the main page has the rail and the chat (its own storage, which the header page shares): the panel sits beside
/// the chat in its corner, or at the right once the chat is moved, as on the Mac.
#[derive(Clone, Copy)]
pub struct PanelPlace {
    pub folded: Option<bool>,
    pub chat: f64,
    pub docked: bool,
}

impl Default for PanelPlace {
    fn default() -> Self { Self { folded: None, chat: 350.0, docked: true } }
}

fn page_of<'a>(engine: &'a Engine, label: &str) -> &'a std::sync::Mutex<BrowserPage> {
    if label == SITE { &engine.browser } else { &engine.side }
}

/// A change in a webview's page, then «look again» to the header that shows it.
fn set(app: &tauri::AppHandle, label: &str, change: impl FnOnce(&mut BrowserPage)) {
    if let Some(engine) = app.try_state::<Engine>() {
        if let Ok(mut page) = page_of(&engine, label).lock() { change(&mut page); }
    }
    let _ = if label == SITE { app.emit_to(EventTarget::webview(BAR), "browser", ()) } else { app.emit_to(EventTarget::webview(PANEL_BAR), "panel", ()) };
}

/// Back or forward: WebView2's own on Windows, which a page's script can't stand in for.
pub fn go(webview: &tauri::Webview, back: bool) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        webview.with_webview(move |w| {
            if let Ok(core) = unsafe { w.controller().CoreWebView2() } {
                let _ = unsafe { if back { core.GoBack() } else { core.GoForward() } };
            }
        })
    }
    #[cfg(not(windows))]
    {
        webview.eval(if back { "history.back()" } else { "history.forward()" })
    }
}

/// WebView2's events for a webview of ours: its history changed (a page's pushState too), a load finished or failed,
/// its page's process ended. The header hears each.
#[cfg(windows)]
pub fn watch(webview: &tauri::Webview, app: &tauri::AppHandle) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{HistoryChangedEventHandler, NavigationCompletedEventHandler, ProcessFailedEventHandler};
    let (label, app) = (webview.label().to_string(), app.clone());
    let _ = webview.with_webview(move |w| {
        let Ok(core) = (unsafe { w.controller().CoreWebView2() }) else { return };
        let (a, l) = (app.clone(), label.clone());
        let history = HistoryChangedEventHandler::create(Box::new(move |sender, _| {
            if let Some(core) = sender { read_history(&a, &l, &core); }
            Ok(())
        }));
        let (a, l) = (app.clone(), label.clone());
        let completed = NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
            if let Some(args) = args {
                let (mut ok, mut status) = (windows::core::BOOL::default(), COREWEBVIEW2_WEB_ERROR_STATUS::default());
                unsafe {
                    let _ = args.IsSuccess(&mut ok);
                    let _ = args.WebErrorStatus(&mut status);
                }
                // A load the next one replaced isn't a failure.
                let error = (!ok.as_bool() && status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED).then(|| reason(status));
                set(&a, &l, |p| p.error = error);
            }
            if let Some(core) = sender { read_history(&a, &l, &core); }
            Ok(())
        }));
        let (a, l) = (app.clone(), label.clone());
        let failed = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(args) = args { unsafe { let _ = args.ProcessFailedKind(&mut kind); } }
            if [COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
                COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED].contains(&kind) {
                set(&a, &l, |p| { p.error = Some("crashed".into()); p.loading = false; });
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            let _ = core.add_HistoryChanged(&history, &mut token);
            let _ = core.add_NavigationCompleted(&completed, &mut token);
            let _ = core.add_ProcessFailed(&failed, &mut token);
        }
    });
}

#[cfg(not(windows))]
pub fn watch(_webview: &tauri::Webview, _app: &tauri::AppHandle) {}

#[cfg(windows)]
fn read_history(app: &tauri::AppHandle, label: &str, core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2) {
    let (mut back, mut forward) = (windows::core::BOOL::default(), windows::core::BOOL::default());
    unsafe {
        let _ = core.CanGoBack(&mut back);
        let _ = core.CanGoForward(&mut forward);
    }
    set(app, label, |p| { p.back = back.as_bool(); p.forward = forward.as_bool(); });
}

/// WebView2's reason, as the header words it (browser-bar.js, browser-panel.js).
#[cfg(windows)]
fn reason(status: webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_ERROR_STATUS) -> String {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let word = match status {
        COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED => "name",
        COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT => "timeout",
        COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT | COREWEBVIEW2_WEB_ERROR_STATUS_SERVER_UNREACHABLE | COREWEBVIEW2_WEB_ERROR_STATUS_DISCONNECTED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED => "connect",
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CLIENT_CERTIFICATE_CONTAINS_ERRORS => "certificate",
        other => return format!("other:{}", other.0),
    };
    word.into()
}

// MARK: The page beside the canvas

/// A web page opened from the canvas or the chat (an explorer, a chart, a transaction, a multisig on Safe), beside the
/// chat in the main window instead of in another app, as the Mac's WebPanel. A plain web view: no wallet and no script
/// of ours in it, and no command of the app's (no capability names it); signing happens only in the app itself. Only
/// web pages load in it: a link to an app (a wallet's deep link, mail) is dropped.
#[tauri::command]
pub async fn open_page(app: tauri::AppHandle, webview: tauri::Webview, url: String) -> Result<(), String> {
    if webview.label() != "main" { return Err("only the main window".into()) }
    let url = tauri::Url::parse(url.trim()).ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .ok_or("Only web pages open beside the canvas.")?;
    if let Some(engine) = app.try_state::<Engine>() {
        if let Ok(mut p) = engine.side.lock() { p.asked = Some(url.clone()); }
    }
    if let Some(page) = app.get_webview(PANEL) {
        page.navigate(url).map_err(|e| e.to_string())?;
        layout_panel(&app);
        return Ok(());
    }
    if let Some(engine) = app.try_state::<Engine>() {
        if let Ok(mut p) = engine.side.lock() {
            let (russian, asked) = (p.russian, p.asked.take());
            *p = BrowserPage { russian, asked, loading: true, ..Default::default() };
        }
    }
    let window = app.get_window("main").ok_or("The main window is closed.")?;
    let (bar, rest) = panel_rects(&app).ok_or("The main window is closed.")?;
    let header = tauri::webview::WebviewBuilder::new(PANEL_BAR, tauri::WebviewUrl::App("browser-panel.html".into()));
    window.add_child(header, tauri::LogicalPosition::new(bar[0], bar[1]), tauri::LogicalSize::new(bar[2], bar[3])).map_err(|e| e.to_string())?;
    let (loads, titles, opens) = (app.clone(), app.clone(), app.clone());
    let page = tauri::webview::WebviewBuilder::new(PANEL, tauri::WebviewUrl::External(url))
        .on_navigation(|u| matches!(u.scheme(), "http" | "https" | "about" | "blob" | "data"))
        // A link that asks for a new window opens here instead.
        .on_new_window(move |u, _| {
            if matches!(u.scheme(), "http" | "https") {
                if let Some(page) = opens.get_webview(PANEL) { let _ = page.navigate(u); }
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |_, payload| {
            let started = matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
            set(&loads, PANEL, |p| { p.loading = started; if started { p.error = None; } });
        })
        .on_document_title_changed(move |_, title| set(&titles, PANEL, |p| p.title = title));
    let page = window.add_child(page, tauri::LogicalPosition::new(rest[0], rest[1]), tauri::LogicalSize::new(rest[2], rest[3])).map_err(|e| e.to_string())?;
    watch(&page, &app);
    Ok(())
}

/// The panel's header and page in the main window, as the Mac's Studio places WebPanel: right of the rail, beside the
/// chat in its corner (at the right when the chat is moved), at most 760 wide and 62% of the room, the window's height.
fn panel_rects(app: &tauri::AppHandle) -> Option<([f64; 4], [f64; 4])> {
    let window = app.get_window("main")?;
    let size = window.inner_size().ok()?.to_logical::<f64>(window.scale_factor().ok()?);
    let place = app.try_state::<Engine>().and_then(|e| e.panel.lock().ok().map(|p| *p)).unwrap_or_default();
    let rail = 10.0 + if place.folded.unwrap_or(size.width < 1140.0) { 92.0 } else { 280.0 };
    let room = (size.width - rail).max(0.0);
    let chat = place.chat.min((room / 2.0).max(320.0));
    let beside = if place.docked { chat + 32.0 } else { 16.0 };
    let width = (room - beside - 16.0).max(380.0).min((((room - beside) * 0.62).max(380.0)).min(760.0)).min((size.width - beside - rail - 8.0).max(240.0));
    // Windows' minimize, maximize and close sit at the top right: a panel reaching there starts under them.
    let top = if place.docked { 16.0 } else { 44.0 };
    let height = (size.height - top - 16.0).max(300.0);
    let x = size.width - beside - width;
    Some(([x, top, width, PANEL_BAR_HEIGHT], [x, top + PANEL_BAR_HEIGHT, width, height - PANEL_BAR_HEIGHT]))
}

/// On a resize of the main window, and when the rail or the chat moved.
pub fn layout_panel(app: &tauri::AppHandle) {
    let Some((bar, rest)) = panel_rects(app) else { return };
    let rect = |r: [f64; 4]| tauri::Rect { position: tauri::LogicalPosition::new(r[0], r[1]).into(), size: tauri::LogicalSize::new(r[2], r[3]).into() };
    if let Some(v) = app.get_webview(PANEL_BAR) { let _ = v.set_bounds(rect(bar)); }
    if let Some(v) = app.get_webview(PANEL) { let _ = v.set_bounds(rect(rest)); }
}

/// The header's report of where the main page has the rail and the chat (its storage changed, or it just loaded).
#[tauri::command]
pub fn page_layout(app: tauri::AppHandle, webview: tauri::Webview, state: tauri::State<'_, Engine>, folded: Option<bool>, chat: Option<f64>,
                   x: Option<f64>, y: Option<f64>) -> Result<(), String> {
    if webview.label() != PANEL_BAR { return Err("only the page's header".into()) }
    *state.panel.lock().map_err(|_| "the engine is gone")? = PanelPlace {
        folded,
        chat: chat.unwrap_or(350.0).clamp(320.0, 4000.0),
        docked: x.unwrap_or(0.0) > -40.0 && y.unwrap_or(0.0) > -40.0,
    };
    layout_panel(&app);
    Ok(())
}

/// The header's buttons: back, forward, reload, stop, close, the page with the wallet (the browser), the page in the
/// person's own browser.
#[tauri::command]
pub async fn page_nav(app: tauri::AppHandle, webview: tauri::Webview, action: String) -> Result<(), String> {
    if webview.label() != PANEL_BAR { return Err("only the page's header".into()) }
    let page = app.get_webview(PANEL).ok_or("No page is open.")?;
    let close = || for label in [PANEL, PANEL_BAR] { if let Some(v) = app.get_webview(label) { let _ = v.close(); } };
    match action.as_str() {
        "back" => go(&page, true).map_err(|e| e.to_string()),
        "forward" => go(&page, false).map_err(|e| e.to_string()),
        "reload" => page.reload().map_err(|e| e.to_string()),
        "stop" => page.eval("window.stop()").map_err(|e| e.to_string()),
        "close" => { close(); Ok(()) }
        // The same page in the browser, where the site can ask for the wallet: the panel gives way to it.
        "wallet" => {
            let url = page.url().map_err(|e| e.to_string())?;
            crate::open_browser(app.clone(), url.to_string()).await?;
            close();
            Ok(())
        }
        "system" => crate::open_link(page.url().map_err(|e| e.to_string())?.to_string()),
        _ => Err(format!("unknown action {action}")),
    }
}

/// What the header draws: the page's address and title, whether it loads, Back, why it didn't load, the language.
#[tauri::command]
pub fn page_state(app: tauri::AppHandle, webview: tauri::Webview, state: tauri::State<'_, Engine>) -> Result<serde_json::Value, String> {
    if webview.label() != PANEL_BAR { return Err("only the page's header".into()) }
    let page = app.get_webview(PANEL).ok_or("No page is open.")?;
    let mut url = page.url().map_err(|e| e.to_string())?;
    let p = state.side.lock().map_err(|_| "the engine is gone")?;
    if !matches!(url.scheme(), "http" | "https") { if let Some(asked) = &p.asked { url = asked.clone(); } }
    Ok(serde_json::json!({ "url": url.as_str(), "title": p.title, "loading": p.loading, "back": p.back || !NATIVE_HISTORY,
                           "forward": p.forward || !NATIVE_HISTORY, "error": p.error, "russian": p.russian }))
}

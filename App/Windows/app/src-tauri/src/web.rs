//! What our webviews of web pages learn from WebView2 itself (Back and Forward as it has them, a page's pushState too;
//! why a page didn't load; a page's process that ended), and the page beside the canvas: the Mac's WebPanel.

use tauri::{Emitter, EventTarget, Manager};

use crate::{BrowserPage, Engine, BAR, BAR_HEIGHT, SITE};

/// Windows reads Back and Forward from WebView2. Elsewhere (a developer's run on a Mac) both stay on, as the Mac's Back is.
pub const NATIVE_HISTORY: bool = cfg!(windows);

/// The page beside the canvas: our header (back, reload, the page's name, copy, with the wallet, the person's browser,
/// close) over a plain web view.
pub const PANEL_BAR: &str = "panel-bar";
pub const PANEL: &str = "panel";
const PANEL_BAR_HEIGHT: f64 = 46.0;
/// The dApp browser in the panel: its toolbar under a row of the panel's own («With your wallet», its own window, close).
pub const PANEL_HEAD: f64 = 40.0;
/// The panel's left edge under its header, the page's full height, which sizes it (the Mac's ResizeEdge): a thin page of
/// our own beside the page, which only reports the width it's dragged to.
pub const PANEL_EDGE: &str = "panel-edge";
const EDGE: f64 = 6.0;

/// Where the main page has the rail and the chat (its own storage, which the header pages share): the panel sits beside
/// the chat in its corner, or at the right once the chat is moved, as on the Mac. Its width (0: its own) and how far its
/// header moved it left of its place are the Mac's webWidth and webX, kept in the same storage.
#[derive(Clone, Copy)]
pub struct PanelPlace {
    pub folded: Option<bool>,
    pub chat: f64,
    pub docked: bool,
    pub width: f64,
    pub offset: f64,
    /// Something fills the main window (Settings, a sheet): the panel is out of sight, as the Mac's under Settings.
    pub hidden: bool,
}

impl Default for PanelPlace {
    fn default() -> Self { Self { folded: None, chat: 350.0, docked: true, width: 0.0, offset: 0.0, hidden: false } }
}

/// The dApp browser shows in the panel: its site is a webview of the main window, not of its own window.
pub fn wallet_in_panel(app: &tauri::AppHandle) -> bool {
    app.get_webview(SITE).is_some_and(|site| site.window().label() == "main")
}

/// The panel's page gives way (to the dApp browser, or on the lock).
pub fn close_page(app: &tauri::AppHandle) {
    shown(app, PANEL, false);
    for label in [PANEL, PANEL_BAR] { if let Some(v) = app.get_webview(label) { let _ = v.close(); } }
}

/// Whether the page panel (`PANEL`) or the dApp browser (`SITE`) shows beside the chat now.
pub fn shown(app: &tauri::AppHandle, label: &str, on: bool) {
    if let Some(engine) = app.try_state::<Engine>() {
        if let Ok(mut page) = page_of(&engine, label).lock() { page.shown = on; }
    }
}

fn is_shown(app: &tauri::AppHandle, label: &str) -> bool {
    app.try_state::<Engine>().is_some_and(|e| page_of(&e, label).lock().is_ok_and(|p| p.shown))
}

/// The edge's page, made once in the main window (only from an async command: a webview made inside a synchronous one
/// waits on the event loop it holds, wry #583). It gets one command, the panel's width (capabilities/panel-edge.json).
pub fn ensure_edge(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview(PANEL_EDGE).is_some() { return Ok(()) }
    let window = app.get_window("main").ok_or("The main window is closed.")?;
    let edge = tauri::webview::WebviewBuilder::new(PANEL_EDGE, tauri::WebviewUrl::App("panel-edge.html".into()));
    window.add_child(edge, tauri::LogicalPosition::new(0.0, 0.0), tauri::LogicalSize::new(1.0, 1.0)).map_err(|e| e.to_string())?;
    Ok(())
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
/// its page's process ended. The header hears each. And WebView2's own UI never shows, as the Mac's WKWebView has no
/// handler for any of it and shows nothing: a permission a page asks for (notifications, location, camera, microphone,
/// clipboard…) is denied; an HTTP sign-in is cancelled; a client certificate isn't offered; a frame sent to an app's
/// scheme opens nothing; a certificate error is refused with no «continue» (the toolbar says «certificate»), and no load
/// error gets WebView2's own page; fields get no autofill suggestions.
#[cfg(windows)]
pub fn watch(webview: &tauri::Webview, app: &tauri::AppHandle) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{BasicAuthenticationRequestedEventHandler, ClientCertificateRequestedEventHandler, HistoryChangedEventHandler,
                       LaunchingExternalUriSchemeEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
                       PermissionRequestedEventHandler, ProcessFailedEventHandler, ServerCertificateErrorDetectedEventHandler, take_pwstr};
    use windows::core::{Interface, PWSTR};
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
                // A load the next one replaced isn't a failure, nor one the app stopped itself (an http link, a phishing
                // host, a check it couldn't make): that one keeps the line it set.
                if status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                    let error = (!ok.as_bool()).then(|| reason(status));
                    set(&a, &l, |p| p.error = error);
                }
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
        // A frame's request that its own handler doesn't take comes here too.
        let permission = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args { unsafe { let _ = args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY); } }
            Ok(())
        }));
        let sign_in = BasicAuthenticationRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args { unsafe { let _ = args.SetCancel(true); } }
            Ok(())
        }));
        // Answered with no certificate and no picker, as a WKWebView that doesn't take the challenge.
        let client_cert = ClientCertificateRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args { unsafe { let _ = args.SetHandled(true); } }
            Ok(())
        }));
        // The top level never gets here (on_navigation refuses an app's scheme); a frame's would get «open this app?».
        let app_link = LaunchingExternalUriSchemeEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args { unsafe { let _ = args.SetCancel(true); } }
            Ok(())
        }));
        // The page's own load, kept so a certificate error on it (and not on a picture inside) says so in the toolbar; a
        // cancelled load ends «cancelled», which keeps the line it set.
        let loading = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let started = {
            let loading = loading.clone();
            NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let mut uri = PWSTR::null();
                if let Some(args) = args { unsafe { let _ = args.Uri(&mut uri); } }
                if let Ok(mut l) = loading.lock() { *l = take_pwstr(uri); }
                Ok(())
            }))
        };
        let (a, l) = (app.clone(), label.clone());
        let bad_cert = ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            unsafe {
                let _ = args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL);
                let _ = args.RequestUri(&mut uri);
            }
            if loading.lock().map(|p| *p == take_pwstr(uri)).unwrap_or(false) { set(&a, &l, |p| p.error = Some("certificate".into())); }
            Ok(())
        }));
        if let Ok(settings) = unsafe { core.Settings() } {
            let _ = unsafe { settings.SetIsBuiltInErrorPageEnabled(false) };
            if let Ok(more) = settings.cast::<ICoreWebView2Settings4>() {
                unsafe { let _ = more.SetIsGeneralAutofillEnabled(false); let _ = more.SetIsPasswordAutosaveEnabled(false); }
            }
        }
        let mut token = 0i64;
        unsafe {
            let _ = core.add_HistoryChanged(&history, &mut token);
            let _ = core.add_NavigationCompleted(&completed, &mut token);
            let _ = core.add_ProcessFailed(&failed, &mut token);
            let _ = core.add_PermissionRequested(&permission, &mut token);
            let _ = core.add_NavigationStarting(&started, &mut token);
            if let Ok(core) = core.cast::<ICoreWebView2_5>() { let _ = core.add_ClientCertificateRequested(&client_cert, &mut token); }
            if let Ok(core) = core.cast::<ICoreWebView2_10>() { let _ = core.add_BasicAuthenticationRequested(&sign_in, &mut token); }
            if let Ok(core) = core.cast::<ICoreWebView2_14>() { let _ = core.add_ServerCertificateErrorDetected(&bad_cert, &mut token); }
            if let Ok(core) = core.cast::<ICoreWebView2_18>() { let _ = core.add_LaunchingExternalUriScheme(&app_link, &mut token); }
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
    // One panel at a time, as the Mac's: the dApp browser beside the chat gives way to the page, kept alive behind it
    // (its page and connection) until the next «open» brings it back.
    shown(&app, SITE, false);
    shown(&app, PANEL, true);
    ensure_edge(&app)?;
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
    let (bar, rest, _, _, _) = panel_rects(&app, PANEL_BAR_HEIGHT).ok_or("The main window is closed.")?;
    let header = tauri::webview::WebviewBuilder::new(PANEL_BAR, tauri::WebviewUrl::App("browser-panel.html".into()));
    window.add_child(header, tauri::LogicalPosition::new(bar[0], bar[1]), tauri::LogicalSize::new(bar[2], bar[3])).map_err(|e| e.to_string())?;
    let (loads, titles, opens) = (app.clone(), app.clone(), app.clone());
    let page = tauri::webview::WebviewBuilder::new(PANEL, tauri::WebviewUrl::External(url))
        // No file picker, alerts, print preview, close() or Notification, as the Mac's panel (keys.rs).
        .initialization_script_for_all_frames(crate::keys::SITE_GUARD)
        .on_navigation(|u| matches!(u.scheme(), "http" | "https" | "about" | "blob" | "data"))
        // A link that asks for a new window opens here instead.
        .on_new_window(move |u, _| {
            if matches!(u.scheme(), "http" | "https") {
                if let Some(page) = opens.get_webview(PANEL) { let _ = page.navigate(u); }
            }
            tauri::webview::NewWindowResponse::Deny
        })
        // Nothing is saved to disk from a page here, as the Mac's panel has no downloads.
        .on_download(|_, _| false)
        .on_page_load(move |_, payload| {
            let started = matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
            set(&loads, PANEL, |p| { p.loading = started; if started { p.error = None; } });
        })
        .on_document_title_changed(move |_, title| set(&titles, PANEL, |p| p.title = title));
    let page = window.add_child(page, tauri::LogicalPosition::new(rest[0], rest[1]), tauri::LogicalSize::new(rest[2], rest[3])).map_err(|e| e.to_string())?;
    watch(&page, &app);
    layout_panel(&app);
    Ok(())
}

/// The panel's header (`head` tall), page and the edge beside the page in the main window, as the Mac's Studio places
/// WebPanel: right of the rail, beside the chat in its corner (at the right when the chat is moved), its own width at most
/// 760 and 62% of the room or the one its edge was dragged to, moved left by its header no further than the rail, the
/// window's height. Also the width and offset it takes, which the header keeps.
fn panel_rects(app: &tauri::AppHandle, head: f64) -> Option<([f64; 4], [f64; 4], [f64; 4], f64, f64)> {
    let window = app.get_window("main")?;
    let size = window.inner_size().ok()?.to_logical::<f64>(window.scale_factor().ok()?);
    let place = app.try_state::<Engine>().and_then(|e| e.panel.lock().ok().map(|p| *p)).unwrap_or_default();
    let rail = 10.0 + if place.folded.unwrap_or(size.width < 1140.0) { 92.0 } else { 280.0 };
    let room = (size.width - rail).max(0.0);
    let chat = place.chat.min((room / 2.0).max(320.0));
    let beside = if place.docked { chat + 32.0 } else { 16.0 };
    let widest = (room - beside - 16.0).max(380.0);
    let own = if place.width > 0.0 { place.width.clamp(380.0, widest) } else { widest.min((((room - beside) * 0.62).max(380.0)).min(760.0)) };
    let width = own.min((size.width - beside - rail - 8.0).max(240.0));
    // Windows' minimize, maximize and close sit at the top right: a panel reaching there starts under them.
    let top = if place.docked { 16.0 } else { 44.0 };
    let height = (size.height - top - 16.0).max(300.0);
    let home = size.width - beside - width;
    let offset = place.offset.min(0.0).max((rail + 8.0 - home).min(0.0));
    let x = home + offset;
    Some(([x, top, width, head], [x + EDGE, top + head, width - EDGE, height - head], [x, top + head, EDGE, height - head], width, offset))
}

/// On a resize of the main window, when the rail or the chat moved, and as the panel's header moves or resizes it: the
/// page with its header, or the dApp browser with its toolbar.
pub fn layout_panel(app: &tauri::AppHandle) {
    let place = app.try_state::<Engine>().and_then(|e| e.panel.lock().ok().map(|p| *p)).unwrap_or_default();
    let in_panel = wallet_in_panel(app);
    // The page on top when one is open, else the browser unless it's put away; the other one hides.
    let showing = if is_shown(app, PANEL) {
        Some(((PANEL_BAR, PANEL_BAR_HEIGHT), PANEL))
    } else if in_panel && is_shown(app, SITE) {
        let bar = app.try_state::<Engine>().and_then(|e| e.browser.lock().ok().map(|p| p.bar)).unwrap_or(BAR_HEIGHT + PANEL_HEAD);
        Some(((BAR, bar), SITE))
    } else {
        None
    };
    let hide = |label: &str| if let Some(v) = app.get_webview(label) { let _ = v.hide(); };
    if in_panel && showing.is_none_or(|(_, page)| page != SITE) { hide(BAR); hide(SITE); }
    let Some((top, page)) = showing else { hide(PANEL_EDGE); return };
    let Some((bar, rest, edge, _, _)) = panel_rects(app, top.1) else { return };
    let rect = |r: [f64; 4]| tauri::Rect { position: tauri::LogicalPosition::new(r[0], r[1]).into(), size: tauri::LogicalSize::new(r[2], r[3]).into() };
    for (label, r) in [(top.0, bar), (page, rest), (PANEL_EDGE, edge)] {
        let Some(v) = app.get_webview(label) else { continue };
        if place.hidden { let _ = v.hide(); } else { let _ = v.set_bounds(rect(r)); let _ = v.show(); }
    }
}

/// A header's report of where the main page has the rail and the chat (its storage changed, or it just loaded), and of
/// the panel's width and offset (its edge or header dragged). The answer is the width and offset the panel took.
#[tauri::command]
pub fn page_layout(app: tauri::AppHandle, webview: tauri::Webview, state: tauri::State<'_, Engine>, folded: Option<bool>, chat: Option<f64>,
                   x: Option<f64>, y: Option<f64>, width: Option<f64>, offset: Option<f64>, hidden: Option<bool>) -> Result<serde_json::Value, String> {
    // The page's header, the browser's toolbar while the browser is in the panel, or the panel's edge.
    if ![PANEL_BAR, BAR, PANEL_EDGE].contains(&webview.label()) { return Err("only the panel's header".into()) }
    let finite = |v: Option<f64>| v.filter(|v| v.is_finite());
    *state.panel.lock().map_err(|_| "the engine is gone")? = PanelPlace {
        folded,
        chat: finite(chat).unwrap_or(350.0).clamp(320.0, 4000.0),
        docked: finite(x).unwrap_or(0.0) > -40.0 && finite(y).unwrap_or(0.0) > -40.0,
        width: finite(width).unwrap_or(0.0).clamp(0.0, 8000.0),
        offset: finite(offset).unwrap_or(0.0).clamp(-8000.0, 0.0),
        hidden: hidden == Some(true),
    };
    layout_panel(&app);
    let head = if webview.label() == BAR { state.browser.lock().map(|p| p.bar).unwrap_or(BAR_HEIGHT) } else { PANEL_BAR_HEIGHT };
    let (_, _, _, width, offset) = panel_rects(&app, head).ok_or("The main window is closed.")?;
    Ok(serde_json::json!({ "width": width, "offset": offset }))
}

/// The header's buttons: back, forward, reload, stop, close, the page with the wallet (the browser), the page in the
/// person's own browser.
#[tauri::command]
pub async fn page_nav(app: tauri::AppHandle, webview: tauri::Webview, action: String) -> Result<(), String> {
    if webview.label() != PANEL_BAR { return Err("only the page's header".into()) }
    let page = app.get_webview(PANEL).ok_or("No page is open.")?;
    // The browser put away behind the page stays put away: the next «open» brings it back, as on the Mac.
    let close = || { close_page(&app); layout_panel(&app); };
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

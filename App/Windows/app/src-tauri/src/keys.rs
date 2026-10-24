//! The Mac's app-menu keys in every window, and WebView2's own browser parts off in the app's own pages.
//!
//! On the Mac ⌘Q, ⌥⌘Q, ⌘L and ⌘, are menu commands, so they work from the dApp browser's window or a page beside the chat
//! as well. Here each webview is its own keyboard target, so the keys are caught at WebView2's level in every one of
//! them, the sites included, before a page can see or swallow them.

/// What a key with Ctrl does anywhere in the app.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Shortcut {
    /// Ctrl+Q: every window goes away, the app stays in the tray (the Mac's ⌘Q, Quit to Menu Bar).
    PutAway,
    /// Ctrl+Shift+Q: quits for real (⌥⌘Q, Quit Completely).
    Quit,
    /// Ctrl+L: locks, with a code set (⌘L); the engine checks that.
    Lock,
    /// Ctrl+,: Settings (⌘,).
    Settings,
}

/// The shortcut a key press is, by its virtual key and the modifiers held. Never with Alt: Ctrl+Alt is AltGr on many
/// layouts, which types characters.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn shortcut(key: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Shortcut> {
    const Q: u32 = 0x51;
    const L: u32 = 0x4C;
    const COMMA: u32 = 0xBC; // VK_OEM_COMMA
    if !ctrl || alt { return None }
    match (key, shift) {
        (Q, false) => Some(Shortcut::PutAway),
        (Q, true) => Some(Shortcut::Quit),
        (L, false) => Some(Shortcut::Lock),
        (COMMA, false) => Some(Shortcut::Settings),
        _ => None,
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn run(app: &tauri::AppHandle, shortcut: Shortcut) {
    use tauri::Emitter;
    match shortcut {
        Shortcut::PutAway => crate::system::put_away(app),
        Shortcut::Quit => crate::system::quit(app),
        Shortcut::Lock => crate::system::tell_engine(app, serde_json::json!({ "op": "lockApp" })),
        Shortcut::Settings => {
            crate::show(app);
            let _ = app.emit_to("main", "engine", r#"{"event":"openView","data":{"view":"settings"}}"#);
        }
    }
}

/// Run on every webview as it's made: the keys above, and in the app's own pages (not a site, not a page beside the chat)
/// no WebView2 context menu and no browser keys (F5 and Ctrl+R reload, Ctrl+P prints, Ctrl+F finds), which the Mac's
/// window never has. A development build keeps those for its tools.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("keys").on_webview_ready(|webview| settle(&webview)).build()
}

#[cfg(windows)]
fn settle(webview: &tauri::Webview) {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows::core::Interface;
    use tauri::Manager;
    let own = ![crate::SITE, crate::web::PANEL].contains(&webview.label());
    let app = webview.app_handle().clone();
    let _ = webview.with_webview(move |w| {
        let controller = w.controller();
        if own && !cfg!(debug_assertions) {
            if let Ok(settings) = unsafe { controller.CoreWebView2() }.and_then(|core| unsafe { core.Settings() }) {
                let _ = unsafe { settings.SetAreDefaultContextMenusEnabled(false) };
                if let Ok(more) = settings.cast::<ICoreWebView2Settings3>() { let _ = unsafe { more.SetAreBrowserAcceleratorKeysEnabled(false) }; }
            }
        }
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let (mut kind, mut key) = (COREWEBVIEW2_KEY_EVENT_KIND::default(), 0u32);
            unsafe {
                args.KeyEventKind(&mut kind)?;
                args.VirtualKey(&mut key)?;
            }
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN { return Ok(()) }
            let held = |vk: i32| unsafe { GetKeyState(vk) } < 0;
            if let Some(s) = shortcut(key, held(VK_CONTROL.0 as i32), held(VK_SHIFT.0 as i32), held(VK_MENU.0 as i32)) {
                unsafe { args.SetHandled(true)?; }
                // Off WebView2's callback: hiding or closing windows from inside it can wait on it.
                let app = app.clone();
                tauri::async_runtime::spawn(async move { run(&app, s) });
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe { let _ = controller.add_AcceleratorKeyPressed(&handler, &mut token); }
    });
}

#[cfg(not(windows))]
fn settle(_webview: &tauri::Webview) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macs_menu_keys_and_nothing_else() {
        assert_eq!(shortcut(0x51, true, false, false), Some(Shortcut::PutAway));
        assert_eq!(shortcut(0x51, true, true, false), Some(Shortcut::Quit));
        assert_eq!(shortcut(0x4C, true, false, false), Some(Shortcut::Lock));
        assert_eq!(shortcut(0xBC, true, false, false), Some(Shortcut::Settings));
        // Without Ctrl, or with AltGr (Ctrl+Alt), a key types what it types.
        assert_eq!(shortcut(0x51, false, false, false), None);
        assert_eq!(shortcut(0x4C, true, false, true), None);
        // Ctrl+Shift+L and Ctrl+C aren't ours.
        assert_eq!(shortcut(0x4C, true, true, false), None);
        assert_eq!(shortcut(0x43, true, false, false), None);
    }
}

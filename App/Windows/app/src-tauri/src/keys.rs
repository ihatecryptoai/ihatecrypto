//! The Mac's app-menu keys in every window; WebView2's own browser parts off in the app's own pages, its menu kept only
//! where the Mac has a text menu; and in the sites no alert and no file picker, as the Mac's web views have neither.
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

/// Run on every webview as it's made: the keys above; in the app's own pages (not a site, not a page beside the chat) no
/// browser keys (F5 and Ctrl+R reload, Ctrl+P prints, Ctrl+F finds) and WebView2's menu only in a field or on selected
/// text, which the Mac's window has that way (a development build keeps all of them for its tools); in a site and a page
/// beside the chat no alert, confirm or prompt.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("keys").on_webview_ready(|webview| settle(&webview)).build()
}

/// In every frame of a site and of a page beside the chat: a file input and the File System Access pickers open nothing,
/// as the Mac's web views have no open panel (no UI delegate), so a page can't ask for a «keystore backup» and read it.
// ponytail: a page script, so a page that reaches a realm without it (a fresh about:blank frame) or a closed shadow root
// it declares in HTML could still ask; WebView2 has no file-chooser event to refuse it at the host. Dropping a file onto
// the page stays, as it needs the person's drag.
pub const NO_FILE_PICKER: &str = r#"(() => {
  const file = (el) => el instanceof HTMLInputElement && el.type === "file";
  const stop = (e) => { if (file(e.composedPath()[0])) { e.preventDefault(); e.stopImmediatePropagation(); } };
  addEventListener("click", stop, true);
  // Inside a closed shadow root the window never sees the input itself: each root gets the same guard.
  const attach = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function (init) { const root = attach.call(this, init); root.addEventListener("click", stop, true); return root; };
  const show = HTMLInputElement.prototype.showPicker;
  if (show) HTMLInputElement.prototype.showPicker = function () { if (file(this)) throw new DOMException("No file picker here.", "NotAllowedError"); return show.call(this); };
  for (const name of ["showOpenFilePicker", "showSaveFilePicker", "showDirectoryPicker"]) {
    if (name in window) window[name] = () => Promise.reject(new DOMException("No file picker here.", "AbortError"));
  }
})();"#;

/// WebView2's menu items kept in a field or on selected text: the Mac's text menu (spelling, Cut, Copy, Paste…).
#[cfg(windows)]
const EDITING: [&str; 9] = ["spellCheck", "emoji", "undo", "redo", "cut", "copy", "paste", "pasteAndMatchStyle", "selectAll"];

#[cfg(windows)]
fn settle(webview: &tauri::Webview) {
    use webview2_com::{AcceleratorKeyPressedEventHandler, ScriptDialogOpeningEventHandler};
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows::core::Interface;
    use tauri::Manager;
    let own = ![crate::SITE, crate::web::PANEL].contains(&webview.label());
    let app = webview.app_handle().clone();
    let _ = webview.with_webview(move |w| {
        let controller = w.controller();
        if let Some((core, settings)) = unsafe { controller.CoreWebView2() }.ok().and_then(|c| unsafe { c.Settings() }.ok().map(|s| (c, s))) {
            if !own {
                // Each alert returns at once, a confirm or a prompt as cancelled, and a page's «leave this page?» lets it
                // go, as WebKit does with no UI delegate.
                let _ = unsafe { settings.SetAreDefaultScriptDialogsEnabled(false) };
                let leave = ScriptDialogOpeningEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_SCRIPT_DIALOG_KIND::default();
                    unsafe { args.Kind(&mut kind)?; }
                    if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD { unsafe { args.Accept()?; } }
                    Ok(())
                }));
                let mut token = 0i64;
                unsafe { let _ = core.add_ScriptDialogOpening(&leave, &mut token); }
            } else if !cfg!(debug_assertions) {
                if let Ok(more) = settings.cast::<ICoreWebView2Settings3>() { let _ = unsafe { more.SetAreBrowserAcceleratorKeysEnabled(false) }; }
                // A runtime without the menu event gets no menu at all.
                if !text_menu_only(&core) { let _ = unsafe { settings.SetAreDefaultContextMenusEnabled(false) }; }
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

/// WebView2's own menu in a page of ours: in a field or on selected text, with its editing items alone; anywhere else none,
/// since the page draws its own (a block's, a canvas's). False when the runtime has no such event.
#[cfg(windows)]
fn text_menu_only(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2) -> bool {
    use webview2_com::{ContextMenuRequestedEventHandler, take_pwstr};
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::{BOOL, Interface, PWSTR};
    let Ok(core) = core.cast::<ICoreWebView2_11>() else { return false };
    let handler = ContextMenuRequestedEventHandler::create(Box::new(|_, args| {
        let Some(args) = args else { return Ok(()) };
        let target = unsafe { args.ContextMenuTarget()? };
        let (mut editable, mut selected) = (BOOL::default(), BOOL::default());
        unsafe {
            target.IsEditable(&mut editable)?;
            target.HasSelection(&mut selected)?;
        }
        if !editable.as_bool() && !selected.as_bool() { return unsafe { args.SetHandled(true) } }
        let items = unsafe { args.MenuItems()? };
        let mut count = 0u32;
        unsafe { items.Count(&mut count)? };
        // From the end, so each removal leaves the indexes still to read as they were.
        for i in (0..count).rev() {
            let mut name = PWSTR::null();
            unsafe { items.GetValueAtIndex(i)?.Name(&mut name)? };
            if !EDITING.contains(&take_pwstr(name).as_str()) { unsafe { items.RemoveValueAtIndex(i)? } }
        }
        unsafe { items.Count(&mut count)? };
        if count == 0 { unsafe { args.SetHandled(true)? } }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { core.add_ContextMenuRequested(&handler, &mut token) }.is_ok()
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

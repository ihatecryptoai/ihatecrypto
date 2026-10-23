fn main() {
    // Each of the app's commands runs only where a capability grants it: the main window gets its own, and pages in the
    // browser get `dapp_call` alone (capabilities/browser.json), so a site can never reach the engine's other requests. The
    // browser's toolbar, our own page beside the site, gets only its three (capabilities/browser-bar.json), and the tray's panel only its four (capabilities/tray.json).
    const COMMANDS: &[&str] = &["engine_send", "protect", "open_link", "copy_secret", "update_check", "update_install", "listen", "open_browser", "dapp_call",
        "browser_nav", "browser_state", "browser_layout", "open_page", "page_nav", "page_state", "page_layout",
        "tray_layout", "tray_state", "tray_open", "tray_quit", "hide_main", "quit_app", "open_notification_settings", "listen_live", "listen_stop"];
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("tauri build");
}

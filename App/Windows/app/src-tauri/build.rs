fn main() {
    // Each of the app's commands runs only where a capability grants it: the main window gets its own, and pages in the
    // browser get `dapp_call` alone (capabilities/browser.json), so a site can never reach the engine's other requests.
    const COMMANDS: &[&str] = &["engine_send", "protect", "open_link", "copy_secret", "update_check", "update_install", "listen", "open_browser", "dapp_call"];
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("tauri build");
}

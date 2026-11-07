import AppKit
#if canImport(Sparkle)
import Sparkle
#endif

/// Updates for the copy downloaded from the site (release.sh). Sparkle reads the site's appcast once a day and checks
/// each update's EdDSA signature and the feed's. It never downloads or installs by itself (SUAllowsAutomaticUpdates is
/// off in the plist): a small window says the new version is out, or a notification in the background, and the user
/// presses Install. The window has no notes (SUShowReleaseNotes is off): What's new shows them after the update. A copy built on this Mac has no feed (SUFeedURL is empty) and never looks. «check for updates» in
/// the chat or the app menu looks now.
/// `swift run` has no Sparkle and no bundle: nothing here runs there.
final class Updater: NSObject {
    static let shared = Updater()
    private weak var workspace: Workspace?
    #if canImport(Sparkle)
    private var controller: SPUStandardUpdaterController?
    #endif

    /// A feed and a key in the app's plist: this copy looks for updates.
    var available: Bool {
        let info = Bundle.main.infoDictionary ?? [:]
        return !(info["SUFeedURL"] as? String ?? "").isEmpty && !(info["SUPublicEDKey"] as? String ?? "").isEmpty
    }

    func start(_ ws: Workspace) {
        guard available else { return }
        workspace = ws
        // Paused for the last update's install (below); a local build never pauses it, so it never resumes it either.
        KeepRunning.resume()
        #if canImport(Sparkle)
        if controller == nil {
            controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: self, userDriverDelegate: self)
            controller?.updater.automaticallyChecksForUpdates = true
        }
        #endif
    }

    /// Sparkle's window: «You're up to date», or the new version with Install.
    func check() {
        #if canImport(Sparkle)
        NSApp.activate()
        controller?.checkForUpdates(nil)
        #endif
    }
}

#if canImport(Sparkle)
extension Updater: SPUUpdaterDelegate, @preconcurrency SPUStandardUserDriverDelegate {
    /// The keep-running agent could open the old copy while Sparkle swaps it: it rests until the new copy starts.
    func updaterWillRelaunchApplication(_ updater: SPUUpdater) { KeepRunning.pause() }

    /// In the background (menu bar, window closed) no window jumps up: a notification says an update is ready, and its
    /// click brings Sparkle's window (NoticeDelegate).
    var supportsGentleScheduledUpdateReminders: Bool { true }

    func standardUserDriverShouldHandleShowingScheduledUpdate(_ update: SUAppcastItem, andInImmediateFocus immediateFocus: Bool) -> Bool {
        immediateFocus
    }

    /// Found: it stays offered by the logo and in the menus, not only in one notification — a gentle reminder holds
    /// Sparkle's session open and no other check runs, so 0.7.0 sat two days with 0.7.7 out and nothing on screen
    /// (the owner: «почему не показывает иконку с обновлением», 2026-10-26).
    func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
        workspace?.updateReady = item.displayVersionString
    }

    func updaterDidNotFindUpdate(_ updater: SPUUpdater) { workspace?.updateReady = nil }

    /// «Skip This Version» drops it; «Remind Me Later» keeps the tag.
    func updater(_ updater: SPUUpdater, userDidMake choice: SPUUserUpdateChoice, forUpdate updateItem: SUAppcastItem, state: SPUUserUpdateState) {
        if choice == .skip { workspace?.updateReady = nil }
    }

    func standardUserDriverWillHandleShowingUpdate(_ handleShowingUpdate: Bool, forUpdate update: SUAppcastItem, state: SPUUserUpdateState) {
        workspace?.updateReady = update.displayVersionString
        guard !handleShowingUpdate, !state.userInitiated, let ws = workspace else { return }
        ws.notify(.update, ws.say("ihatecrypto \(update.displayVersionString) is ready", "Готова ihatecrypto \(update.displayVersionString)"),
                  ws.say("Click to see what's new and install it.", "Нажми, чтобы посмотреть, что нового, и установить."), record: false)
    }
}
#endif

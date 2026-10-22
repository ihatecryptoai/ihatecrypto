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

    func standardUserDriverWillHandleShowingUpdate(_ handleShowingUpdate: Bool, forUpdate update: SUAppcastItem, state: SPUUserUpdateState) {
        guard !handleShowingUpdate, !state.userInitiated, let ws = workspace else { return }
        ws.notify(.update, ws.say("ihatecrypto \(update.displayVersionString) is ready", "Готова ihatecrypto \(update.displayVersionString)"),
                  ws.say("Click to see what's new and install it.", "Нажми, чтобы посмотреть, что нового, и установить."), record: false)
    }
}
#endif

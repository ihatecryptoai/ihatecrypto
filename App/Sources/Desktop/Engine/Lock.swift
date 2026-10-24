import CWinSec
import Foundation
import SignerAPI
import WalletCore

/// The app's lock, as the Mac's AppLock (Passcode.swift): with a passcode set, the window shows only the lock screen at
/// launch, after `lockAfter` minutes away from the PC, when it sleeps or its session locks, on «lock» and Ctrl+L. The
/// signer checks the code (`.enterCode`) and turns it off after five wrong ones; Windows Hello opens the lock too. Only
/// the window is hidden: the schedule, stops and alerts go on. While locked the window gets none of the balances, the
/// chat or the history: the engine sends it the lock alone.
nonisolated struct LockView: Encodable, Equatable {
    /// A passcode is set: the signer's word, kept in the settings so a launch is locked before the signer answers.
    var codeSet = false
    var locked = false
    /// Five wrong codes turned the code off: only Windows Hello opens the app — after a restart too.
    var codeOff = false
    /// Why the last code or Windows Hello didn't open it («Wrong code · 3 tries left»).
    var problem: String?
    /// Minutes away before it locks: 1, 5, 15 or 60.
    var lockAfter = 5
    var checking = false
    /// Windows Hello stood in for a forgotten code: Settings › Wallet opens on making a new one.
    var offerNewCode = false
    /// Scheduled tasks keep running while it's locked: the lock screen says so.
    var tasksRun = false
}

/// What the window gets while it's locked: the lock and the language, nothing else.
nonisolated struct LockedSnapshot: Encodable {
    var lock: LockView
    var russian: Bool
    var language: String
}

extension Engine {
    static let lockChoices = [1, 5, 15, 60]
    /// Requests the window may make while locked: the lock's own, and what starts it.
    static let whileLocked: Set<String> = ["hello", "unlock", "unlockHello", "signerPid"]

    var lockView: LockView {
        var v = lock
        v.codeSet = settings.codeSet == true
        v.codeOff = settings.codeOff == true
        v.lockAfter = Self.lockChoices.contains(settings.lockAfter ?? 5) ? settings.lockAfter ?? 5 : 5
        v.tasksRun = !tasks.isEmpty
        return v
    }

    var lockedSnapshot: LockedSnapshot { LockedSnapshot(lock: lockView, russian: russian, language: settings.language ?? "auto") }

    /// Before the signer has answered: locked if the last run had a code, or had it turned off by five wrong ones.
    func lockAtLaunch() {
        lock.locked = settings.codeSet == true || settings.codeOff == true
        startLockWatch()
    }

    /// The signer's `codeSet` after each status read. Its first answer after launch locks a window the settings left open.
    func lockHeard(codeSet set: Bool) {
        settings.codeSet = set
        if set {
            settings.codeOff = nil
            if !lockHeardSigner { lock.locked = true }
        } else if lock.locked {
            // No code any more while locked (five wrong codes, or the signer couldn't read it): only Windows Hello opens it.
            settings.codeOff = true
        }
        lockHeardSigner = true
    }

    /// Hides the window, ends the signing session; the window closes the browser (the `locked` event).
    func lockApp() async {
        guard settings.codeSet == true else { return }
        lock.locked = true
        lock.problem = nil
        // The window closes the browser: a site's request waiting is answered no (the Mac's sheet goes with its page).
        declineSite(say("Locked before an answer: nothing was signed.", "Заблокировано до ответа: ничего не подписано."))
        out.event("locked", [String: String]())
        _ = try? await SignerLink.shared.send(.endSession)
        changed()
    }

    /// The code on the lock screen: the signer checks it and counts the wrong ones; it opens no signing session.
    func unlock(code: String) async {
        guard lock.locked, !lock.checking else { return }
        lock.checking = true
        changed()
        defer { lock.checking = false; changed() }
        guard let r = try? await SignerLink.shared.send(.enterCode(code: code, sign: false)) else {
            lock.problem = say("The signer didn't answer: try again, or use Windows Hello.", "Подписант не ответил: попробуй ещё раз или открой через Windows Hello.")
            return
        }
        if r.ok { openLock() }
        else if r.codeOff == true { settings.codeOff = true; settings.codeSet = false; lock.problem = nil }
        else { lock.problem = r.error.map(localizedCodeError) }
    }

    /// Windows Hello in its own window: ihatecrypto never sees the PIN, the face or the fingerprint. After a code that's
    /// off, Settings › Wallet then offers a new one.
    func unlockWithHello() async {
        guard lock.locked else { return }
        let reason = say("open ihatecrypto", "открыть ihatecrypto")
        var signature: UnsafeMutablePointer<UInt8>?
        var count = 0
        var challenge = [UInt8](repeating: 0, count: 32)
        for i in challenge.indices { challenge[i] = .random(in: 0...255) }
        // The window lets this process, and Hello's dialog, come forward, as it does for the signer's.
        out.event("signerForeground", ["pid": "\(ProcessInfo.processInfo.processIdentifier)"])
        // A key of the engine's own: a signature from it means Windows Hello saw the person; nothing is kept from it.
        let status = Array(Self.unlockKey.utf8).withUnsafeBufferPointer { name in
            challenge.withUnsafeBufferPointer { c in
                Array(reason.utf8).withUnsafeBufferPointer { r in
                    winsec_hello_sign(name.baseAddress, name.count, c.baseAddress, c.count, r.baseAddress, r.count, 1, &signature, &count)
                }
            }
        }
        if let signature { winsec_free(signature, count) }
        guard status == WINSEC_OK else {
            if status != WINSEC_CANCELLED {
                lock.problem = say("Windows Hello didn't open it: set up a PIN in Windows Settings › Accounts › Sign-in options.",
                                   "Windows Hello не открыл: задай PIN в «Параметры Windows › Учётные записи › Варианты входа».")
            }
            changed()
            return
        }
        let wasOff = settings.codeOff == true
        openLock()
        if wasOff {
            settings.codeOff = nil
            lock.offerNewCode = true
        }
        changed()
    }

    private func openLock() {
        lock.locked = false
        lock.problem = nil
    }

    /// Makes or changes the code; the signer asks Windows Hello first (signerCall reads its status again after).
    func makeCode(_ code: String) async throws {
        if let problem = Passcode.problem(code) { throw WalletError(localizedCodeError(problem)) }
        do { try await signerCall(.setCode(code: code)) } catch { throw WalletError(localizedCodeError(error.localizedDescription)) }
        lock.offerNewCode = false
        changed()
    }

    func turnCodeOff() async throws {
        try await signerCall(.clearCode)
        settings.codeOff = nil
        changed()
    }

    func setLockAfter(_ minutes: Int) {
        guard Self.lockChoices.contains(minutes) else { return }
        settings.lockAfter = minutes
        changed()
    }

    /// The signer's words for a code, in the window's language.
    func localizedCodeError(_ text: String) -> String {
        guard russian else { return text }
        if let left = text.wholeMatch(of: /Wrong code · (\d+) tr(y|ies) left/)?.1 { return "Неверный код · осталось попыток: \(left)" }
        return [
            "The code is 6 digits.": "Код — 6 цифр.",
            "All the same digit is the first code anyone tries.": "Одну и ту же цифру пробуют первой.",
            "A straight run like 123456 is guessed first.": "Ряд вроде 123456 угадывают первым.",
            "The signer couldn't count this try, so it didn't check the code: try again.": "Подписант не смог засчитать попытку и не проверил код: попробуй ещё раз.",
        ][text] ?? text
    }

    /// Every 15 s: time away from the PC, its session locked, or a gap in the timer (the PC slept) locks the app.
    private var sessionEndedForLock: Bool {
        get { Self.sessionEnded }
        set { Self.sessionEnded = newValue }
    }
    nonisolated(unsafe) private static var sessionEnded = false

    private func startLockWatch() {
        Task {
            var last = Date()
            while true {
                try? await Task.sleep(for: .seconds(15))
                let now = Date()
                let slept = now.timeIntervalSince(last) > 75
                last = now
                let screenLocked = winsec_session_locked() == 1
                if slept || screenLocked, !sessionEndedForLock { sessionEndedForLock = true; _ = try? await SignerLink.shared.send(.endSession) }
                if !slept && !screenLocked { sessionEndedForLock = false }
                guard settings.codeSet == true, !lock.locked else { continue }
                let away = Int(winsec_idle_ms()) >= lockView.lockAfter * 60_000
                if slept || away || winsec_session_locked() == 1 { await lockApp() }
            }
        }
    }

    /// The engine's own Windows Hello key, apart from the signer's: one per copy of the app.
    #if DEBUG
    static let unlockKey = "ihatecrypto-dev-unlock"
    #else
    static let unlockKey = "ihatecrypto-unlock"
    #endif
}

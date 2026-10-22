import AppKit
import LocalAuthentication
import SignerAPI
import SwiftUI
import WalletCore

// MARK: - The lock

/// The app's lock:
/// with a passcode set, the window shows only the lock screen at launch, after `lockAfter` minutes away from the Mac, on
/// sleep or a locked screen, on «lock» and ⌘L. The signer checks the code (`.enterCode`) and turns it off after five
/// wrong ones; Touch ID and the Mac password open the lock too. Only the window is hidden: the schedule, stops and alerts
/// go on.
@MainActor @Observable
final class AppLock {
    static let shared = AppLock()
    static let codeSetKey = "codeSet", lockAfterKey = "lockAfter", codeOffKey = "codeOff"
    /// Minutes away before it locks.
    static let choices = [1, 5, 15, 60]

    /// A passcode is set: the signer's word, kept in defaults so a launch is locked before the signer answers.
    private(set) var codeSet = UserDefaults.standard.bool(forKey: codeSetKey)
    private(set) var locked = UserDefaults.standard.bool(forKey: codeSetKey) || UserDefaults.standard.bool(forKey: codeOffKey)
    /// Why the last code didn't open it («Wrong code · 3 tries left»).
    var problem: String?
    /// Five wrong codes turned the code off: only the Mac password opens the app — after a relaunch too.
    private(set) var codeOff = UserDefaults.standard.bool(forKey: codeOffKey) {
        didSet { if !previewing { UserDefaults.standard.set(codeOff, forKey: Self.codeOffKey) } }
    }
    private(set) var checking = false
    /// The Mac password stood in for a forgotten code: Settings › Wallet opens on making a new one.
    var offerNewCode = false
    var lockAfter = AppLock.storedLockAfter {
        didSet { UserDefaults.standard.set(lockAfter, forKey: Self.lockAfterKey) }
    }
    /// On a Mac without Touch ID the code signs moves too: the signer asks for it instead of the Mac password.
    var codeSigns: Bool { codeSet && !Presence.touchID }

    private static var storedLockAfter: Int {
        let minutes = UserDefaults.standard.integer(forKey: lockAfterKey)
        return choices.contains(minutes) ? minutes : 5
    }

    private weak var ws: Workspace?
    private var timer: Timer?
    private var heardSigner = false
    /// `-previewLock locked|wrong|off|sign` (Debug only): a state drawn without a code, to look at it.
    private var previewing = false

    func start(_ ws: Workspace) {
        guard self.ws == nil else { return }
        self.ws = ws
        SignerClient.askCode = { reason, problem in await CodePrompt.shared.ask(reason, problem) }
        let lockNow: @Sendable (Notification) -> Void = { _ in Task { @MainActor in AppLock.shared.lock() } }
        DistributedNotificationCenter.default().addObserver(forName: .init("com.apple.screenIsLocked"), object: nil, queue: .main, using: lockNow)
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main, using: lockNow)
        // Away = no keyboard or mouse anywhere on the Mac, not just in this window.
        timer = Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { _ in
            MainActor.assumeIsolated { AppLock.shared.lockIfAway() }
        }
        #if DEBUG
        let preview = UserDefaults.standard.string(forKey: "previewLock")
        previewing = preview != nil
        switch preview {
        case "locked": locked = true
        case "wrong": (locked, problem) = (true, "Wrong code · 3 tries left")
        case "off": (locked, codeOff) = (true, true)
        case "sign": CodePrompt.shared.preview(.init(reason: "send 5 USDC to Bybit on Solana: 5 USDC → 9WzD…AWWM", problem: nil))
        default: break
        }
        #endif
    }

    private func lockIfAway() {
        guard codeSet, !locked, let any = CGEventType(rawValue: ~0) else { return }
        if CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: any) >= Double(lockAfter * 60) { lock() }
    }

    /// The signer's `codeSet` (Workspace.loadSigner). Its first answer after launch locks a window the defaults left open.
    func update(codeSet set: Bool) {
        guard !previewing else { return }
        if set != codeSet {
            codeSet = set
            UserDefaults.standard.set(set, forKey: Self.codeSetKey)
        }
        if set {
            codeOff = false
            if !heardSigner { locked = true }
        } else if locked {
            // No code any more while the window is locked (five wrong codes, or the signer couldn't read it): only the
            // Mac password opens it. A status reply never does.
            codeOff = true
        }
        heardSigner = true
    }

    /// Hides the window, ends the signing session and closes the browser and import windows.
    func lock() {
        guard codeSet else { return }
        locked = true
        problem = nil
        CodePrompt.shared.reply(nil)
        for window in NSApp.windows where ["browser", "import"].contains(where: { window.identifier?.rawValue.hasPrefix($0) == true }) {
            window.close()
        }
        if SignerClient.isAvailable { Task { _ = try? await SignerClient().send(.endSession) } }
    }

    /// The code on the lock screen: the signer checks it and counts the wrong ones; it opens no signing session.
    func enter(_ code: String) async {
        guard !checking else { return }
        checking = true
        defer { checking = false }
        guard SignerClient.isAvailable, let r = try? await SignerClient().send(.enterCode(code: code, sign: false)) else {
            problem = "The signer didn't answer: try again, or use your Mac password."
            return
        }
        if r.ok { open() }
        else if r.codeOff == true { codeOff = true; problem = nil; update(codeSet: false) }
        else { problem = r.error }
    }

    func unlockWithTouchID() async {
        if await evaluate(.deviceOwnerAuthenticationWithBiometrics) { open() }
    }

    /// macOS asks in its own window; ihatecrypto never sees the password. Then Settings › Wallet offers a new code.
    func unlockWithMacPassword() async {
        guard await evaluate(.deviceOwnerAuthentication) else { return }
        open()
        codeOff = false
        offerNewCode = true
        ws?.openSettings(.wallet)
    }

    private func evaluate(_ policy: LAPolicy) async -> Bool {
        do { return try await LAContext().evaluatePolicy(policy, localizedReason: "open ihatecrypto") }
        catch {
            if (error as? LAError)?.code != .userCancel, (error as? LAError)?.code != .appCancel { problem = error.localizedDescription }
            return false
        }
    }

    private func open() {
        locked = false
        problem = nil
    }

    /// Makes or changes the code; the signer asks for Touch ID or the Mac password first. nil when it's done.
    func make(_ code: String) async -> String? {
        if let problem = Passcode.problem(code) { return problem }
        guard SignerClient.isAvailable else { return "This build has no signer." }
        do {
            let r = try await SignerClient().send(.setCode(code: code))
            guard r.ok else { return r.error ?? "The code wasn't saved." }
            update(codeSet: true)
            offerNewCode = false
            return nil
        } catch { return error.localizedDescription }
    }

    func turnOff() async -> String? {
        guard SignerClient.isAvailable else { return nil }
        do {
            let r = try await SignerClient().send(.clearCode)
            guard r.ok else { return r.error ?? "The code is still on." }
            update(codeSet: false)
            return nil
        } catch { return error.localizedDescription }
    }
}

extension Presence {
    /// What signs a move here: Touch ID, the passcode on a Mac without it, otherwise the Mac password.
    @MainActor static var signName: String { AppLock.shared.codeSigns ? "your code" : name }
    @MainActor static var signShort: String { AppLock.shared.codeSigns ? "Code" : short }
    @MainActor static var signIcon: String { AppLock.shared.codeSigns ? "circle.grid.3x3.fill" : icon }
    /// «Подпиши Touch ID / кодом / паролем от Mac».
    @MainActor static var signRuIns: String { AppLock.shared.codeSigns ? "кодом" : ruIns }
}

// MARK: - The code a signature waits for

/// A signature the signer won't make without the passcode (`SignerClient.askCode`): the chat's field turns into the code
/// row (`CodeEntryRow`), or a card when Settings or the onboarding fill the window. One at a time: a new ask cancels one
/// still open.
@MainActor @Observable
final class CodePrompt {
    static let shared = CodePrompt()
    struct Ask: Hashable {
        /// What the signer will sign, in its own words.
        let reason: String
        /// Why the last code failed.
        let problem: String?
    }
    private(set) var pending: Ask?
    private var answer: CheckedContinuation<String?, Never>?

    func ask(_ reason: String, _ problem: String?) async -> String? {
        reply(nil)
        if AppLock.shared.locked { return nil }
        StatusItem.shared.openMain()
        return await withCheckedContinuation { continuation in
            answer = continuation
            pending = Ask(reason: reason, problem: problem)
        }
    }

    #if DEBUG
    func preview(_ ask: Ask) { pending = ask }
    #endif

    /// The code, or nil: cancelled.
    func reply(_ code: String?) {
        pending = nil
        let waiting = answer
        answer = nil
        waiting?.resume(returning: code)
    }
}

// MARK: - Pieces

private enum LockLook {
    static let ground = Color(light: 0xF6F6F4, dark: 0x0E0F12)
    static let field = Color(light: 0xFFFFFF, dark: 0x1C1E23)
    static let rim = Color(light: 0xD5D6DA, dark: 0xFFFFFF, lightAlpha: 1, darkAlpha: 0.12)
}

/// Six digits into a secure field (macOS keeps other apps from reading the keys while it has focus), drawn as boxes or
/// dots. Anything but digits is dropped; the sixth calls `done`.
struct CodeDigits: View {
    enum Style { case boxes, dots }
    @Binding var code: String
    var style: Style = .boxes
    var error = false
    var done: () -> Void = {}
    @FocusState private var focused: Bool

    var body: some View {
        ZStack {
            SecureField("", text: $code)
                .textFieldStyle(.plain).focused($focused).opacity(0.02).frame(width: 10, height: 10)
                .accessibilityLabel("Passcode, \(Passcode.length) digits")
                .onSubmit { if code.count == Passcode.length { done() } }
            HStack(spacing: style == .boxes ? 10 : 12) {
                ForEach(0..<Passcode.length, id: \.self) { i in
                    let filled = i < code.count, next = i == code.count && focused
                    switch style {
                    case .boxes:
                        RoundedRectangle(cornerRadius: 11)
                            .fill(LockLook.field)
                            .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(error ? Brand.loss : next ? Brand.accent : LockLook.rim, lineWidth: next || error ? 1.6 : 1))
                            .overlay { if filled { Circle().fill(Color.primary).frame(width: 11, height: 11) } }
                            .frame(width: 46, height: 54)
                    case .dots:
                        Circle().fill(filled ? Brand.accent : .clear)
                            .overlay(Circle().strokeBorder(filled ? Brand.accent : error ? Brand.loss : Color.primary.opacity(0.3), lineWidth: 2))
                            .frame(width: 13, height: 13)
                    }
                }
            }
            .accessibilityHidden(true)
            .contentShape(Rectangle())
            .onTapGesture { focused = true }
        }
        .onAppear { focused = true }
        .onChange(of: code) { _, new in
            let digits = String(new.filter(\.isASCII).filter(\.isNumber).prefix(Passcode.length))
            if digits != new { code = digits }
            if digits.count == Passcode.length, new.count == Passcode.length { done() }
        }
    }
}

/// 1 min · 5 min · 15 min · 1 hour, and always on sleep.
struct LockAfterPicker: View {
    var body: some View {
        let lock = AppLock.shared
        HStack(spacing: 6) {
            ForEach(AppLock.choices, id: \.self) { minutes in
                let on = lock.lockAfter == minutes
                Button { lock.lockAfter = minutes } label: {
                    Text(minutes == 60 ? "1 hour" : "\(minutes) min").font(.ui(12, on ? .semibold : .regular))
                        .foregroundStyle(on ? Brand.accentInk : Color.primary)
                        .padding(.horizontal, 11).frame(height: 28)
                        .background(RoundedRectangle(cornerRadius: 8).fill(on ? Brand.accent.opacity(0.12) : .clear))
                        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(on ? Brand.accent : Brand.cardBorder, lineWidth: on ? 1.5 : 1))
                        .contentShape(RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain).pointerStyle(.link).accessibilityAddTraits(on ? [.isSelected] : [])
            }
        }
    }
}

/// The two steps of making a code: type it, then once more. `made` runs once the signer has it.
struct CodeMaker: View {
    var made: () -> Void
    @State private var code = ""
    @State private var first: String?
    @State private var problem: String?
    @State private var busy = false

    var step: String { first == nil ? "STEP 1 OF 2 · TYPE IT" : "STEP 2 OF 2 · ONCE MORE" }
    var ready: Bool { code.count == Passcode.length && !busy }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(step).font(.ui(11, .semibold)).tracking(0.8).foregroundStyle(.secondary)
            HStack(spacing: 12) {
                CodeDigits(code: $code, error: problem != nil) { Task { await next() } }
                    .id(first == nil)
                if busy { ProgressView().controlSize(.small) }
            }
            if let problem { Text(problem).font(.ui(12.5, .medium)).foregroundStyle(Brand.loss) }
            else if first != nil {
                Label("Then macOS asks for \(Presence.name) once, to save the code.", systemImage: Presence.icon)
                    .font(.ui(12.5)).foregroundStyle(.secondary)
            }
            HStack(spacing: 10) {
                Text("Lock after").font(.ui(12.5)).foregroundStyle(.secondary)
                LockAfterPicker()
                Text("and always on sleep").font(.ui(12)).foregroundStyle(.secondary)
            }
        }
    }

    /// Step 1 keeps the code; step 2 must match it, then the signer asks for Touch ID or the password and saves it.
    func next() async {
        guard ready else { return }
        guard let first else {
            if let p = Passcode.problem(code) { problem = p; code = ""; return }
            (self.first, code, problem) = (code, "", nil)
            return
        }
        guard code == first else { (self.first, code, problem) = (nil, "", "The two codes differ: type a new one."); return }
        busy = true
        defer { busy = false }
        if let p = await AppLock.shared.make(code) { (self.first, code, problem) = (nil, "", p); return }
        made()
    }
}

// MARK: - The lock screen

/// Variant A with its states: the code field and Touch ID; a wrong code; the code off after five.
struct LockScreen: View {
    @Environment(Workspace.self) private var ws
    @State private var code = ""
    @FocusState private var focused: Bool

    var body: some View {
        let lock = AppLock.shared
        ZStack {
            LockLook.ground.ignoresSafeArea()
            VStack(spacing: 24) {
                ZStack(alignment: .bottomTrailing) {
                    BrandLogo(size: 108, mood: lock.problem != nil ? .error : .asleep)
                    Image(systemName: "lock.fill").font(.system(size: 15, weight: .bold)).foregroundStyle(.primary)
                        .frame(width: 38, height: 38).background(Circle().fill(LockLook.field))
                        .overlay(Circle().strokeBorder(LockLook.ground, lineWidth: 3)).offset(x: 6, y: 4)
                }
                .accessibilityHidden(true)
                VStack(spacing: 8) {
                    Text(lock.codeOff ? "The code is off" : "ihatecrypto is locked").font(.ui(24, .semibold))
                    Text(lock.codeOff ? "5 wrong tries. Only your Mac password opens ihatecrypto now; then make a new code in Settings › Wallet."
                                      : "Balances, chat and history stay hidden until you unlock.")
                        .font(.ui(14.5)).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 440)
                }
                if lock.codeOff { codeOff } else { field }
            }
            .padding(.bottom, 30)
            VStack {
                Spacer()
                ZStack {
                    if !lock.codeOff {
                        HStack(spacing: 5) {
                            Text("Forgot the code?").foregroundStyle(.secondary)
                            Button("Unlock with your Mac password") { Task { await lock.unlockWithMacPassword() } }
                                .buttonStyle(.plain).fontWeight(.semibold).underline(color: .primary.opacity(0.35)).pointerStyle(.link)
                        }
                        .font(.ui(14))
                    }
                    if !ws.scheduleEntries.isEmpty {
                        HStack {
                            Label("Scheduled tasks keep running", systemImage: "clock").font(.ui(12.5)).foregroundStyle(.secondary)
                            Spacer()
                        }
                    }
                }
                .padding(.horizontal, 24).padding(.bottom, 30)
            }
        }
        .onAppear { code = ""; focused = true }
    }

    private var field: some View {
        let lock = AppLock.shared
        return VStack(spacing: 16) {
            VStack(spacing: 9) {
                HStack(spacing: 10) {
                    SecureField("Enter your passcode", text: $code)
                        .textFieldStyle(.plain).font(.ui(16)).focused($focused)
                        .onSubmit(submit)
                        .onChange(of: code) { _, new in
                            let digits = String(new.filter(\.isASCII).filter(\.isNumber).prefix(Passcode.length))
                            if digits != new { code = digits }
                            if digits.count == Passcode.length { submit() }
                        }
                    Button(action: submit) {
                        Group {
                            if lock.checking { ProgressView().controlSize(.small) }
                            else { Image(systemName: "arrow.right").font(.system(size: 15, weight: .bold)) }
                        }
                        .foregroundStyle(Brand.onAccent).frame(width: 40, height: 40).background(Circle().fill(Brand.accent))
                    }
                    .buttonStyle(.plain).pointerStyle(.link).accessibilityLabel("Unlock").disabled(code.count < Passcode.length)
                }
                .padding(.leading, 22).padding(.trailing, 7)
                .frame(width: 380, height: 54)
                .background(Capsule().fill(LockLook.field))
                .overlay(Capsule().strokeBorder(lock.problem != nil ? Brand.loss : LockLook.rim, lineWidth: lock.problem != nil ? 1.5 : 1))
                if let problem = lock.problem { Text(problem).font(.ui(13.5, .medium)).foregroundStyle(Brand.loss) }
            }
            if Presence.touchID {
                HStack(spacing: 14) {
                    Rectangle().fill(Color.primary.opacity(0.14)).frame(height: 1)
                    Text("or").font(.ui(13)).foregroundStyle(.secondary)
                    Rectangle().fill(Color.primary.opacity(0.14)).frame(height: 1)
                }
                .frame(width: 380).accessibilityHidden(true)
                Button { Task { await lock.unlockWithTouchID() } } label: {
                    Label("Unlock with Touch ID", systemImage: "touchid").font(.ui(15.5, .semibold)).foregroundStyle(Brand.onAccent)
                        .padding(.horizontal, 22).frame(height: 48).background(Capsule().fill(Brand.accent)).contentShape(Capsule())
                }
                .buttonStyle(.plain).pointerStyle(.link)
            }
        }
    }

    private var codeOff: some View {
        let lock = AppLock.shared
        return VStack(spacing: 14) {
            Button { Task { await lock.unlockWithMacPassword() } } label: {
                Label("Unlock with your Mac password", systemImage: "lock.fill").font(.ui(15, .semibold)).foregroundStyle(Brand.onAccent)
                    .padding(.horizontal, 22).frame(height: 46).background(Capsule().fill(Brand.accent)).contentShape(Capsule())
            }
            .buttonStyle(.plain).pointerStyle(.link).keyboardShortcut(.defaultAction)
            Text("macOS asks for it in its own window: ihatecrypto never sees the password.").font(.ui(12.5)).foregroundStyle(.secondary)
            if let problem = lock.problem { Text(problem).font(.ui(13, .medium)).foregroundStyle(Brand.loss) }
        }
    }

    private func submit() {
        guard code.count == Passcode.length, !AppLock.shared.checking else { return }
        let typed = code
        Task {
            await AppLock.shared.enter(typed)
            code = ""
            focused = true
        }
    }
}

// MARK: - Signing with the code

/// The chat's field while a signature waits for the passcode (state «Sign with the code»): what the signer will sign,
/// six dots, Cancel and Sign.
struct CodeEntryRow: View {
    let ask: CodePrompt.Ask
    @State private var code = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 14) {
                Text("Code to sign").font(.ui(13.5, .medium)).foregroundStyle(.secondary)
                CodeDigits(code: $code, style: .dots, error: ask.problem != nil, done: sign)
                Spacer(minLength: 6)
                Button("Cancel") { CodePrompt.shared.reply(nil) }
                    .buttonStyle(.plain).font(.ui(12.5, .medium)).padding(.horizontal, 12).frame(height: 32)
                    .overlay(Capsule().strokeBorder(Brand.cardBorder)).pointerStyle(.link).keyboardShortcut(.cancelAction)
                Button(action: sign) {
                    Text("Sign").font(.ui(12.5, .bold)).foregroundStyle(Brand.onAccent)
                        .padding(.horizontal, 16).frame(height: 32).background(Capsule().fill(Brand.accent)).contentShape(Capsule())
                }
                .buttonStyle(.plain).pointerStyle(.link).disabled(code.count < Passcode.length)
            }
            Text(ask.problem ?? ask.reason).font(.ui(11.5, ask.problem == nil ? .regular : .medium))
                .foregroundStyle(ask.problem == nil ? Color.secondary : Brand.loss).lineLimit(2)
                .help(ask.reason)
        }
        .padding(.vertical, 11).padding(.leading, 16).padding(.trailing, 9)
        .id(ask)
    }

    private func sign() {
        guard code.count == Passcode.length else { return }
        CodePrompt.shared.reply(code)
    }
}

/// The same ask as a card, when Settings or the onboarding fill the window and there's no chat field.
struct CodePromptCard: View {
    let ask: CodePrompt.Ask

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("This Mac has no Touch ID, so your code signs instead of the Mac password.").font(.ui(12)).foregroundStyle(.secondary)
                .padding(.horizontal, 16).padding(.top, 12)
            CodeEntryRow(ask: ask)
        }
        .frame(width: 560)
        .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Brand.card).shadow(color: .black.opacity(0.18), radius: 24, y: 10))
        .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).strokeBorder(Brand.accent, lineWidth: 1.5))
    }
}

// MARK: - Settings › Wallet

/// Make, change or turn off the code; how soon it locks; lock now.
struct PasscodeSettings: View {
    @State private var making = false
    @State private var problem: String?
    @State private var busy = false

    var body: some View {
        let lock = AppLock.shared
        SettingsSection(icon: "circle.grid.3x3.fill", title: "Passcode",
                        line: lock.codeSigns ? "Opens ihatecrypto and signs your moves on this Mac." : "Opens ihatecrypto: balances, chat and history stay hidden until then.") {
            SettingsGroup {
                SettingsRow("Passcode", icon: "lock", info: info) {
                    HStack(spacing: 8) {
                        if busy { ProgressView().controlSize(.small) }
                        if lock.codeSet {
                            Text("On").foregroundStyle(.secondary)
                            Button(making ? "Cancel" : "Change…") { making.toggle() }
                            Button("Turn off") {
                                Task {
                                    busy = true
                                    problem = await lock.turnOff()
                                    busy = false
                                }
                            }
                        } else {
                            Button(making ? "Cancel" : "Make a code…") { making.toggle() }
                        }
                    }
                    .disabled(busy)
                }
                if making {
                    CodeMaker { making = false }
                        .padding(.horizontal, 16).padding(.vertical, 14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                if let problem { Text(problem).font(.ui(12.5)).foregroundStyle(Brand.loss).padding(.horizontal, 16).padding(.vertical, 10) }
                if lock.codeSet {
                    SettingsRow("Lock after", icon: "timer", info: "Minutes without touching the Mac's keyboard or mouse. Sleep and a locked screen lock it at once.") {
                        LockAfterPicker()
                    }
                    SettingsRow("Lock the app now", icon: "lock.fill", info: "Hides everything until the code, Touch ID or your Mac password, and ends the signing session. ⌘L does the same.") {
                        Button("Lock now") { lock.lock() }
                    }
                }
            }
        }
        .onAppear { if lock.offerNewCode { making = true } }
    }

    private var info: String {
        "6 digits, checked by the separate signer: five wrong in a row turn it off, and then only your Mac password opens the app. "
            + (Presence.touchID ? "Touch ID opens it too; moves still sign with Touch ID."
                                : "On this Mac it also signs moves instead of the Mac password. Adding an address or changing wallets still asks for the Mac password: a code seen by something on the Mac can't send money anywhere new.")
    }
}

// MARK: - The menu bar

/// The menu bar's popover while the app is locked: no money, only the way back.
struct MenuBarLocked: View {
    var body: some View {
        HStack(spacing: 10) {
            BrandLogo(size: 28, mood: .asleep)
            VStack(alignment: .leading, spacing: 1) {
                Text("ihatecrypto is locked").font(.ui(13, .semibold))
                Text("Scheduled tasks keep running").font(.ui(10)).foregroundStyle(.secondary)
            }
            Spacer()
            Button("Open") { StatusItem.shared.openMain() }
        }
        .padding(14).frame(width: 300)
    }
}

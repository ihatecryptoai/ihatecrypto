import CWinSec
import Foundation
import IhatecryptoCore
import Ledger
import SignerAPI
import TradeExecution
import WalletCore

/// A Ledger plugged in over USB, through winsec.dll's HID calls. Each exchange finds the device anew: it reconnects
/// whenever an app opens or quits on it, so a handle kept from before would be dead.
// ponytail: USB only, as on the Mac's first version; Bluetooth (Nano X, Stax, Flex) needs a BLE transport with the same framing.
nonisolated struct WinLedgerHID: LedgerTransport {
    /// One exchange at a time.
    private static let lock = NSLock()

    /// «Nano S Plus», «Flex»… of the first Ledger plugged in, or nil when none is.
    static func connected() -> String? {
        var name = [UInt8](repeating: 0, count: 64)
        let n = winsec_ledger_product(&name, Int32(name.count))
        return n > 0 ? String(decoding: name.prefix(Int(n)), as: UTF8.self) : nil
    }

    func exchange(_ apdu: Data, timeout: TimeInterval) async throws -> Data {
        try await Task.detached { try Self.exchangeSync(apdu, timeout: timeout) }.value
    }

    static func exchangeSync(_ apdu: Data, timeout: TimeInterval) throws -> Data {
        lock.lock()
        defer { lock.unlock() }
        // Right after an app switch the device is briefly gone: wait for it to come back.
        var device = winsec_ledger_open()
        for _ in 0..<20 where device == nil {
            Thread.sleep(forTimeInterval: 0.25)
            device = winsec_ledger_open()
        }
        guard let device else {
            throw LedgerError(connected() == nil ? "No Ledger found: plug it in over USB and unlock it." : "The Ledger is busy: close Ledger Live or any other wallet using it, then try again.")
        }
        defer { winsec_ledger_close(device) }
        for packet in LedgerFraming.packets(apdu) {
            let written = packet.withUnsafeBytes { winsec_ledger_write(device, $0.bindMemory(to: UInt8.self).baseAddress, Int32(packet.count)) }
            guard written == 0 else { throw LedgerError("Couldn't write to the Ledger: plug it in again.") }
        }
        var packets: [Data] = []
        let deadline = Date().addingTimeInterval(timeout)
        while true {
            if let reply = try LedgerFraming.reply(packets) { return reply }
            let left = deadline.timeIntervalSinceNow
            guard left > 0 else { throw LedgerError("The Ledger didn't answer in time: check its screen and try again.") }
            var buffer = [UInt8](repeating: 0, count: LedgerFraming.hidPacketSize)
            let n = winsec_ledger_read(device, &buffer, Int32(buffer.count), Int32(min(left, 1) * 1000))
            guard n >= 0 else { throw LedgerError("Couldn't read from the Ledger: plug it in again.") }
            if n > 0 { packets.append(Data(buffer.prefix(Int(n)))) }
        }
    }
}

/// A Ledger over Bluetooth LE through winsec.dll's WinRT GATT calls, as the Mac's LedgerBLE: one link for the app's
/// life (linking takes seconds), Ledger's framing without the HID channel, in pieces as large as the link allows.
nonisolated final class WinLedgerBLE: LedgerTransport, @unchecked Sendable {
    static let shared = WinLedgerBLE()
    private let lock = NSLock()
    private var link: UnsafeMutableRawPointer?
    private var mtu = 20
    private(set) var model: String?

    /// Links a Ledger within `timeout`: its model, or nil with the reason.
    func connect(timeout: TimeInterval) async -> (model: String?, why: String?) {
        await Task.detached { [self] in
            lock.withLock {
                do { try linkUp(timeout) } catch { return (nil, error.localizedDescription) }
                return (model, nil)
            }
        }.value
    }

    func exchange(_ apdu: Data, timeout: TimeInterval) async throws -> Data {
        try await Task.detached { [self] in
            try lock.withLock {
                // The first link may wait for Windows' pairing prompt.
                try linkUp(45)
                do { return try exchangeSync(apdu, timeout: timeout) } catch {
                    // A link that went away is made again next time.
                    winsec_ble_close(link)
                    link = nil
                    throw error
                }
            }
        }.value
    }

    private func linkUp(_ timeout: TimeInterval) throws {
        guard link == nil else { return }
        var why = [UInt8](repeating: 0, count: 256), whyLength: Int32 = 0
        guard let l = winsec_ble_open(Int32(timeout * 1000), &why, Int32(why.count), &whyLength) else {
            throw LedgerError(String(decoding: why.prefix(Int(whyLength)), as: UTF8.self))
        }
        var name = [UInt8](repeating: 0, count: 32), size: Int32 = 20
        let n = winsec_ble_info(l, &name, Int32(name.count), &size)
        model = n > 0 ? String(decoding: name.prefix(Int(n)), as: UTF8.self) : "Ledger"
        link = l
        // Ledger's own client confirms notifications (0x0001), then asks the device's MTU (0x08).
        _ = try? request(Data([0x00, 0x01]), timeout: 2)
        if let reply = try? request(Data([0x08, 0, 0, 0, 0]), timeout: 3), reply.count > 5, reply[reply.startIndex] == 0x08 {
            mtu = max(20, min(Int(reply[reply.startIndex + 5]), Int(size)))
        } else {
            mtu = max(20, Int(size))
        }
    }

    private func write(_ frame: Data) throws {
        let sent = frame.withUnsafeBytes { winsec_ble_write(link, $0.bindMemory(to: UInt8.self).baseAddress, Int32(frame.count)) }
        guard sent == 0 else { throw LedgerError("The Ledger went away over Bluetooth: unlock it and keep it close.") }
    }

    private func read(_ timeout: TimeInterval) throws -> Data? {
        var buffer = [UInt8](repeating: 0, count: 512)
        let n = winsec_ble_read(link, &buffer, Int32(buffer.count), Int32(timeout * 1000))
        guard n >= 0 else { throw LedgerError("The Ledger went away over Bluetooth: unlock it and keep it close.") }
        return n > 0 ? Data(buffer.prefix(Int(n))) : nil
    }

    /// One handshake frame and the first notification back.
    private func request(_ frame: Data, timeout: TimeInterval) throws -> Data {
        winsec_ble_drain(link)
        try write(frame)
        guard let reply = try read(timeout) else { throw LedgerError("No answer over Bluetooth.") }
        return reply
    }

    private func exchangeSync(_ apdu: Data, timeout: TimeInterval) throws -> Data {
        winsec_ble_drain(link)
        for frame in LedgerFraming.packets(apdu, size: mtu, hid: false) { try write(frame) }
        var packets: [Data] = []
        let deadline = Date().addingTimeInterval(timeout)
        while true {
            if let reply = try LedgerFraming.reply(packets, hid: false) { return reply }
            let left = deadline.timeIntervalSinceNow
            guard left > 0 else { throw LedgerError("The Ledger didn't answer in time: check its screen and try again.") }
            if let p = try read(min(left, 1)) { packets.append(p) }
        }
    }
}

/// The Ledger's addresses as a wallet in the list: nothing of it is on this computer but what it shows.
nonisolated struct LedgerAccount: Codable, Equatable {
    static let id = "ledger"
    var label = "Ledger"
    var model: String?
    var evm: String?
    var solana: String?
    var evmPath = LedgerDevice.evmPath
    var solanaPath = LedgerDevice.solanaPath
    /// It signs now instead of the signer's wallet.
    var active: Bool?
    /// Found over Bluetooth rather than USB.
    var bluetooth: Bool?

    var transport: any LedgerTransport { bluetooth == true ? WinLedgerBLE.shared : WinLedgerHID() }

    var wallet: SignerWallet { SignerWallet(id: Self.id, label: label, kind: "ledger", solana: solana, evm: evm, near: nil) }
}

/// The connect card's progress on the canvas.
nonisolated struct LedgerConnect: Encodable, Equatable {
    var model: String?
    var evm: String?
    var solana: String?
    var status = "Looking for a Ledger on USB…"
    var problem: String?
    /// Settings on the device to change (blind signing off, an app not installed).
    var notes: [String] = []
    var busy = true
}

extension Engine {
    var ledgerActive: Bool { ledger?.active == true && ledger?.evm != nil }

    /// The signer a move uses while the Ledger is on: the same guard the signer runs checks each request, then the
    /// device shows it and waits for its buttons. What the device waits for shows on the running move.
    func ledgerSigner(_ wallet: TradeWallet) -> LedgerSigner {
        let account = ledger ?? LedgerAccount()
        return LedgerSigner(device: LedgerDevice(account.transport), wallet: wallet, evmPath: account.evmPath, solanaPath: account.solanaPath, model: account.model,
                            prompt: { [weak self] text in
            Task { @MainActor in
                guard let self else { return }
                self.ledgerPrompt = text
                if let text { self.updateBoard(self.runningBoard) { $0.lines.append("Ledger: " + text) } }
                self.changed()
            }
        })
    }

    /// Opens the Ledger's Ethereum and Solana apps in turn and reads one address from each. Nothing is signed; the
    /// addresses become the «Ledger» wallet in the list.
    func connectLedger() async {
        ledgerConnect = LedgerConnect(status: say("Looking for a Ledger on USB…", "Ищу Ledger на USB…"))
        pin(Board(kind: "ledger", title: "ledger"))
        reply(say("Connecting your Ledger — follow its block on the canvas.", "Подключаю Ledger — следи за блоком на холсте."))
        // USB when it's plugged in; otherwise Bluetooth (Nano X, Stax, Flex, Apex), looked for in short rounds.
        var found = WinLedgerHID.connected()
        var bluetooth = false
        var why: String?
        for _ in 0..<12 where found == nil {
            ledgerConnect?.status = say("Plug your Ledger in over USB and unlock it — or, on a Nano X, Stax or Flex, unlock it with Bluetooth on and keep it close.",
                                        "Подключи Ledger по USB и разблокируй — или, если это Nano X, Stax или Flex, разблокируй с включённым Bluetooth и держи рядом.")
            changed()
            let ble = await WinLedgerBLE.shared.connect(timeout: 4)
            if let model = ble.model { found = model; bluetooth = true; break }
            why = ble.why
            try? await Task.sleep(for: .seconds(1))
            guard ledgerConnect != nil else { return }
            found = WinLedgerHID.connected()
        }
        guard let model = found else {
            ledgerConnect?.problem = say("No Ledger answered over USB or Bluetooth in a minute. Plug it in or turn its Bluetooth on, unlock it and press Connect again.",
                                         "За минуту Ledger не ответил ни по USB, ни по Bluetooth. Подключи его или включи Bluetooth, разблокируй и нажми «Подключить снова».")
                + (why.map { " (" + $0 + ")" } ?? "")
            ledgerConnect?.busy = false
            changed()
            return
        }
        ledgerConnect?.model = model + (bluetooth ? " · Bluetooth" : " · USB")
        var account = ledger ?? LedgerAccount()
        account.model = model
        account.bluetooth = bluetooth
        let device = LedgerDevice(account.transport)
        do {
            ledgerConnect?.status = say("Open the Ethereum app on your Ledger — or allow it when the Ledger asks.", "Открой приложение Ethereum на Ledger — или разреши, когда он спросит.")
            changed()
            try await device.open("Ethereum")
            account.evm = try await device.evmAddress(account.evmPath)
            ledgerConnect?.evm = account.evm
            if let blind = try? await device.evmBlindSigning() {
                ledgerConnect?.notes.append("Ethereum \(blind.version): turn on «Raw messages» in the app's settings — then the Ledger shows Hyperliquid approvals and withdrawals field by field."
                    + (blind.on ? "" : " «Blind signing» is needed too for swaps, Aave, Morpho and bridges."))
            }
            ledgerConnect?.status = say("Now the Solana app: allow it when the Ledger asks.", "Теперь приложение Solana: разреши, когда Ledger спросит.")
            changed()
            do {
                try await device.open("Solana")
                account.solana = try await device.solanaAddress(account.solanaPath)
                ledgerConnect?.solana = account.solana
                if let blind = try? await device.solanaBlindSigning(), !blind.on {
                    ledgerConnect?.notes.append("Solana \(blind.version): turn on «Blind signing» in the app's settings for USDC transfers, Jupiter swaps and lending. SOL sends work without it.")
                }
            } catch let e as LedgerError where e.status == 0x6807 {
                ledgerConnect?.notes.append("No Solana app on this Ledger: add it in Ledger Live › My Ledger, then connect again. Until then it signs EVM moves only.")
            }
            ledger = account
            ledgerConnect?.status = ledgerActive ? say("Connected. It signs every move now.", "Подключён. Теперь подписывает каждое движение.")
                                                 : say("Connected. Switch to it to sign with it.", "Подключён. Переключись на него, чтобы подписывать им.")
        } catch {
            if account.evm != nil { ledger = account }
            ledgerConnect?.problem = error.localizedDescription
        }
        ledgerConnect?.busy = false
        changed()
    }

    /// One of its addresses on its own screen, to compare with the one here: what the device says is the truth.
    func showOnLedger(solana: Bool) async {
        guard let account = ledger else { return }
        let device = LedgerDevice(account.transport)
        if ledgerConnect == nil { ledgerConnect = LedgerConnect(model: account.model, evm: account.evm, solana: account.solana, busy: false) }
        pin(Board(kind: "ledger", title: "ledger"))
        ledgerConnect?.problem = nil
        ledgerConnect?.status = say("Compare the address on your Ledger with the one here, then approve it there.", "Сравни адрес на Ledger с адресом здесь и подтверди на устройстве.")
        changed()
        do {
            if solana {
                try await device.open("Solana")
                _ = try await device.solanaAddress(account.solanaPath, show: true)
            } else {
                try await device.open("Ethereum")
                _ = try await device.evmAddress(account.evmPath, show: true)
            }
            ledgerConnect?.status = say("The Ledger showed the same address.", "Ledger показал тот же адрес.")
        } catch {
            ledgerConnect?.problem = error.localizedDescription
        }
        changed()
    }

    /// The Ledger signs from now on (true), or the signer's wallet again.
    func useLedger(_ on: Bool) {
        guard var account = ledger else { return }
        guard account.active != on else { return }
        account.active = on
        ledger = account
        cancelPending()
        portfolio = PortfolioView()
        loadEquity()
        Task { await refresh() }
    }

    /// Takes the Ledger off the list: only its addresses were here.
    func forgetLedger() {
        guard ledger != nil else { reply(say("No Ledger is connected.", "Ledger не подключён.")); return }
        useLedger(false)
        ledger = nil
        ledgerConnect = nil
        reply(say("Ledger removed from the wallet list. The money on it stays on it; connect it again any time.",
                  "Ledger убран из списка кошельков. Деньги на нём остаются на нём; подключить снова можно в любой момент."))
    }

    /// «connect ledger», «подключи леджер», «use my Ledger», «forget my Ledger»: true when the message was about it.
    func ledgerCommand(_ text: String) async -> Bool {
        let lower = text.lowercased()
        guard lower.contains(/ledger|л[еэ]дж[еэ]р/) else { return false }
        if lower.wholeMatch(of: /\s*(?:use|switch\s+to|sign\s+with|используй|переключи\w*\s+на|подписывай\s+(?:с|на))\s+(?:my\s+|мой\s+|моём\s+|мо[йеё]м?\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)\s*[.!]?\s*/) != nil, ledger != nil {
            useLedger(true)
            reply(say("Your Ledger signs now: every move asks for its buttons.", "Теперь подписывает Ledger: каждое движение просит его кнопки."))
            return true
        }
        if lower.wholeMatch(of: /\s*(?:forget|remove|disconnect|забудь|удали|отключи)\s+(?:my\s+|мой\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)\s*[.!]?\s*/) != nil {
            forgetLedger()
            return true
        }
        if lower.wholeMatch(of: /\s*(?:(?:connect|add|pair|set\s*up|link|подключи\w*|добавь|привяжи|подсоедини)\s+(?:my\s+|a\s+|мой\s+)?)?(?:ledger|л[еэ]дж[еэ]р\w*)(?:\s+(?:nano\s*\w*|flex|stax))?\s*[.!?]?\s*/) != nil {
            Task { await connectLedger() }
            return true
        }
        return false
    }
}

import CoreBluetooth
import Foundation

/// A Ledger Nano X, Stax, Flex or Apex over Bluetooth LE: the same APDUs as over USB, framed without the HID channel,
/// in pieces as large as the link allows (the device tells its MTU when asked with an 0x08 frame). One link for the
/// app's life, since connecting takes seconds; the first time, macOS and the Ledger show a pairing code to confirm.
// ponytail: one Ledger at a time — the first one found wins.
public final class LedgerBLE: NSObject, LedgerTransport, @unchecked Sendable {
    public static let shared = LedgerBLE()

    /// Each model's GATT service (LedgerHQ device-sdk-ts): notify is …-0001-…, write …-0002-…, write command …-0003-….
    static var models: [(service: CBUUID, name: String)] { [
        (CBUUID(string: "13D63400-2C97-0004-0000-4C6564676572"), "Nano X"),
        (CBUUID(string: "13D63400-2C97-6004-0000-4C6564676572"), "Stax"),
        (CBUUID(string: "13D63400-2C97-3004-0000-4C6564676572"), "Flex"),
        (CBUUID(string: "13D63400-2C97-8004-0000-4C6564676572"), "Apex"),
    ] }
    static func characteristic(_ service: CBUUID, _ n: Int) -> CBUUID {
        CBUUID(string: service.uuidString.replacingOccurrences(of: "-0000-4C65", with: "-000\(n)-4C65"))
    }

    /// Delegate callbacks land here; the exchanges wait on `work`, so neither blocks the other.
    private let callbacks = DispatchQueue(label: "ledger.ble.callbacks")
    private let work = DispatchQueue(label: "ledger.ble")
    /// Guards everything below; callbacks change it and broadcast, exchanges wait on it.
    private let state = NSCondition()
    private var central: CBCentralManager?
    private var peripheral: CBPeripheral?
    private var writer: CBCharacteristic?
    private var linked = false
    private var failure: String?
    private var inbox: [Data] = []
    private var wrote = false
    private var mtu = 20
    /// «Nano X», «Flex»… of the Ledger on the link.
    public private(set) var model: String?

    public func exchange(_ apdu: Data, timeout: TimeInterval) async throws -> Data {
        try await withCheckedThrowingContinuation { c in
            work.async { c.resume(with: Result { try self.exchangeSync(apdu, timeout: timeout) }) }
        }
    }

    /// Finds and links a Ledger within `timeout`; the model's name, or nil when none answered.
    public func connect(timeout: TimeInterval) async -> String? {
        await withCheckedContinuation { c in
            work.async { c.resume(returning: (try? self.link(timeout: timeout)) != nil ? self.model : nil) }
        }
    }

    // MARK: Link

    /// Waits until `ready` holds or the time is up; a failure reported by a callback ends the wait with it.
    private func wait(_ timeout: TimeInterval, _ what: String, until ready: () -> Bool) throws {
        let deadline = Date().addingTimeInterval(timeout)
        state.lock()
        defer { state.unlock() }
        while !ready() {
            if let failure { self.failure = nil; throw LedgerError(failure) }
            guard state.wait(until: deadline) else { throw LedgerError(what) }
        }
    }

    private func link(timeout: TimeInterval) throws {
        state.lock()
        let up = linked && peripheral?.state == .connected
        failure = nil
        state.unlock()
        if up { return }
        callbacks.sync {
            if central == nil { central = CBCentralManager(delegate: self, queue: callbacks) }
        }
        try wait(5, "Bluetooth didn't start on this Mac.") { central?.state != .unknown && central?.state != .resetting }
        switch central?.state {
        case .poweredOn: break
        case .unauthorized: throw LedgerError("ihatecrypto may not use Bluetooth: allow it in System Settings › Privacy & Security › Bluetooth.")
        case .poweredOff: throw LedgerError("Bluetooth is off on this Mac: turn it on to reach the Ledger.")
        default: throw LedgerError("This Mac has no Bluetooth LE.")
        }
        callbacks.async { [self] in
            guard let central else { return }
            let services = Self.models.map(\.service)
            // Linked by the system already (paired, awake): use it; otherwise look for one advertising.
            if let known = central.retrieveConnectedPeripherals(withServices: services).first {
                peripheral = known
                known.delegate = self
                central.connect(known)
            } else {
                central.scanForPeripherals(withServices: services)
            }
        }
        do {
            try wait(timeout, "No Ledger answered over Bluetooth: unlock it, turn its Bluetooth on and keep it close.") { linked }
        } catch {
            callbacks.async { [self] in
                central?.stopScan()
                if let peripheral { central?.cancelPeripheralConnection(peripheral) }
            }
            throw error
        }
        // Ledger's own client first asks the device to confirm notifications (0x0001 → five zero bytes), then its MTU.
        _ = try? request(Data([0x00, 0x01]), timeout: 2)
        if let reply = try? request(Data([0x08, 0, 0, 0, 0]), timeout: 3), reply.count > 5, reply[reply.startIndex] == 0x08 {
            let link = callbacks.sync { peripheral?.maximumWriteValueLength(for: .withoutResponse) ?? 20 }
            state.lock()
            mtu = max(20, min(Int(reply[reply.startIndex + 5]), link))
            state.unlock()
        }
    }

    /// One raw frame out and the first notification back (the handshake frames, not APDUs).
    private func request(_ frame: Data, timeout: TimeInterval) throws -> Data {
        state.lock()
        inbox.removeAll()
        state.unlock()
        try send(frame)
        try wait(timeout, "No answer over Bluetooth.") { !inbox.isEmpty }
        state.lock()
        defer { state.unlock() }
        return inbox.removeFirst()
    }

    private func send(_ packet: Data) throws {
        state.lock()
        wrote = false
        state.unlock()
        let withResponse = callbacks.sync { () -> Bool in
            guard let peripheral, let writer else { return false }
            let type: CBCharacteristicWriteType = writer.properties.contains(.writeWithoutResponse) ? .withoutResponse : .withResponse
            peripheral.writeValue(packet, for: writer, type: type)
            return type == .withResponse
        }
        if withResponse { try wait(10, "The Ledger didn't take the data over Bluetooth.") { wrote } }
    }

    private func exchangeSync(_ apdu: Data, timeout: TimeInterval) throws -> Data {
        // The first link may wait for the pairing code to be confirmed on both screens.
        try link(timeout: 45)
        state.lock()
        inbox.removeAll()
        let size = mtu
        state.unlock()
        for packet in LedgerFraming.packets(apdu, size: size, hid: false) { try send(packet) }
        var reply: Data?
        try wait(timeout, "The Ledger didn't answer in time: check its screen and try again.") {
            reply = try? LedgerFraming.reply(inbox, hid: false) ?? nil
            return reply != nil
        }
        return reply!
    }
}

extension LedgerBLE: CBCentralManagerDelegate, CBPeripheralDelegate {
    private func changed(_ body: () -> Void) {
        state.lock()
        body()
        state.broadcast()
        state.unlock()
    }

    public func centralManagerDidUpdateState(_ central: CBCentralManager) { changed {} }

    public func centralManager(_ central: CBCentralManager, didDiscover peripheral: CBPeripheral, advertisementData: [String: Any], rssi: NSNumber) {
        central.stopScan()
        self.peripheral = peripheral
        peripheral.delegate = self
        central.connect(peripheral)
    }

    public func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        peripheral.discoverServices(Self.models.map(\.service))
    }

    public func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral, error: Error?) {
        changed { failure = "The Ledger didn't connect over Bluetooth: \(error?.localizedDescription ?? "unknown reason")." }
    }

    public func centralManager(_ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral, error: Error?) {
        changed {
            linked = false
            writer = nil
            failure = "The Ledger went away over Bluetooth: unlock it and keep it close."
        }
    }

    public func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard let service = peripheral.services?.first(where: { s in Self.models.contains { $0.service == s.uuid } }) else {
            changed { failure = "That Bluetooth device isn't a Ledger." }
            return
        }
        changed { model = Self.models.first { $0.service == service.uuid }?.name }
        peripheral.discoverCharacteristics((1...3).map { Self.characteristic(service.uuid, $0) }, for: service)
    }

    public func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService, error: Error?) {
        let chars = service.characteristics ?? []
        guard let notify = chars.first(where: { $0.uuid == Self.characteristic(service.uuid, 1) }),
              let writer = chars.first(where: { $0.uuid == Self.characteristic(service.uuid, 3) }) ?? chars.first(where: { $0.uuid == Self.characteristic(service.uuid, 2) }) else {
            changed { failure = "The Ledger's Bluetooth service is missing its channels: update its firmware." }
            return
        }
        changed { self.writer = writer }
        peripheral.setNotifyValue(true, for: notify)
    }

    public func peripheral(_ peripheral: CBPeripheral, didUpdateNotificationStateFor characteristic: CBCharacteristic, error: Error?) {
        changed {
            if let error { failure = "The Ledger refused the Bluetooth link: \(error.localizedDescription). Pair it again in System Settings › Bluetooth." }
            else { linked = true }
        }
    }

    public func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
        guard let value = characteristic.value else { return }
        changed { inbox.append(value) }
    }

    public func peripheral(_ peripheral: CBPeripheral, didWriteValueFor characteristic: CBCharacteristic, error: Error?) {
        changed {
            if let error { failure = "Writing to the Ledger over Bluetooth failed: \(error.localizedDescription)." }
            wrote = true
        }
    }
}

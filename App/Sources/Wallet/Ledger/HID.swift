import Foundation
import IOKit.hid

/// A Ledger plugged in over USB. Each exchange finds the device anew: it reconnects whenever an app opens or quits,
/// so a handle kept from before would be dead.
// ponytail: USB only; Nano X / Flex / Stax over Bluetooth need a CoreBluetooth transport with the same framing.
public struct LedgerHID: LedgerTransport {
    static let vendor = 0x2c97
    /// The HID interface that carries APDUs (the others are U2F/FIDO and WebUSB).
    static let usagePage = 0xffa0
    /// One exchange at a time, on a thread whose run loop delivers the device's replies.
    static let queue = DispatchQueue(label: "ledger.hid")

    public init() {}

    /// «Nano S Plus», «Nano X»… of the first Ledger plugged in, or nil when none is. Only lists devices, so it
    /// doesn't wait behind an exchange that waits for the person.
    public static func connected() -> String? {
        guard let device = find() else { return nil }
        let product = IOHIDDeviceGetProperty(device, kIOHIDProductIDKey as CFString) as? Int ?? 0
        return switch product >> 8 {
        case 0x10: "Nano S"
        case 0x40: "Nano X"
        case 0x50: "Nano S Plus"
        case 0x60: "Stax"
        case 0x70: "Flex"
        default: IOHIDDeviceGetProperty(device, kIOHIDProductKey as CFString) as? String ?? "Ledger"
        }
    }

    public func exchange(_ apdu: Data, timeout: TimeInterval) async throws -> Data {
        try await withCheckedThrowingContinuation { c in
            Self.queue.async { c.resume(with: Result { try Self.exchangeSync(apdu, timeout: timeout) }) }
        }
    }

    static func find() -> IOHIDDevice? {
        let manager = IOHIDManagerCreate(kCFAllocatorDefault, IOOptionBits(kIOHIDOptionsTypeNone))
        IOHIDManagerSetDeviceMatching(manager, [kIOHIDVendorIDKey: vendor, kIOHIDPrimaryUsagePageKey: usagePage] as CFDictionary)
        guard let set = IOHIDManagerCopyDevices(manager) as? Set<IOHIDDevice> else { return nil }
        return set.first
    }

    /// Replies land here from the device's run-loop callback.
    final class Inbox {
        var packets: [Data] = []
    }

    static func exchangeSync(_ apdu: Data, timeout: TimeInterval) throws -> Data {
        // Right after an app switch the device is briefly gone: wait for it to come back.
        var found = find()
        for _ in 0..<20 where found == nil {
            Thread.sleep(forTimeInterval: 0.25)
            found = find()
        }
        guard let device = found else { throw LedgerError("No Ledger found: plug it in over USB and unlock it.") }
        guard IOHIDDeviceOpen(device, IOOptionBits(kIOHIDOptionsTypeNone)) == kIOReturnSuccess else {
            throw LedgerError("The Ledger is busy: close Ledger Live or any other wallet using it, then try again.")
        }
        defer { IOHIDDeviceClose(device, IOOptionBits(kIOHIDOptionsTypeNone)) }

        let inbox = Inbox()
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: LedgerFraming.hidPacketSize)
        let context = Unmanaged.passRetained(inbox)
        IOHIDDeviceRegisterInputReportCallback(device, buffer, LedgerFraming.hidPacketSize, { context, _, _, _, _, report, length in
            guard let context else { return }
            Unmanaged<Inbox>.fromOpaque(context).takeUnretainedValue().packets.append(Data(bytes: report, count: length))
        }, context.toOpaque())
        let runLoop: CFRunLoop = CFRunLoopGetCurrent()!
        IOHIDDeviceScheduleWithRunLoop(device, runLoop, CFRunLoopMode.defaultMode.rawValue)
        defer {
            IOHIDDeviceUnscheduleFromRunLoop(device, runLoop, CFRunLoopMode.defaultMode.rawValue)
            IOHIDDeviceRegisterInputReportCallback(device, buffer, LedgerFraming.hidPacketSize, nil, nil)
            context.release()
            buffer.deallocate()
        }

        for packet in LedgerFraming.packets(apdu) {
            let sent = packet.withUnsafeBytes {
                IOHIDDeviceSetReport(device, kIOHIDReportTypeOutput, 0, $0.bindMemory(to: UInt8.self).baseAddress!, packet.count)
            }
            guard sent == kIOReturnSuccess else { throw LedgerError("Couldn't write to the Ledger (\(sent)): plug it in again.") }
        }
        let deadline = Date().addingTimeInterval(timeout)
        while true {
            if let reply = try LedgerFraming.reply(inbox.packets) { return reply }
            guard Date() < deadline else { throw LedgerError("The Ledger didn't answer in time: check its screen and try again.") }
            CFRunLoopRunInMode(.defaultMode, 0.05, true)
        }
    }
}

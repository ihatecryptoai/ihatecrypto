import Foundation
import WalletCore

/// A Ledger talks in APDUs: a command (class, instruction, two parameters, up to 255 bytes) and a reply that ends in a
/// two-byte status word. The keys never leave the device; every signature is shown on its screen and approved with its
/// buttons. Over USB the APDUs go in 64-byte HID packets (`LedgerHID`); the Speculos emulator takes them over TCP.
public protocol LedgerTransport: Sendable {
    /// Sends one APDU and returns the reply with its status word. `timeout`: how long the person may take on the device.
    func exchange(_ apdu: Data, timeout: TimeInterval) async throws -> Data
}

public struct LedgerError: LocalizedError, Equatable {
    public let status: UInt16?
    public let message: String
    public init(_ message: String, status: UInt16? = nil) { (self.message, self.status) = (message, status) }
    public var errorDescription: String? { message }

    public var declined: Bool { status == 0x6985 || status == 0x5501 }
    /// Another app, or the dashboard, is open instead of the one the command was for.
    public var wrongApp: Bool { [0x6d00, 0x6e00, 0x6e01, 0x6511, 0x6a82].contains(status ?? 0) }

    /// What a status word means for the person holding the device, in plain words.
    static func from(_ status: UInt16, app: String) -> LedgerError {
        let text = switch status {
        case 0x6985, 0x5501: "You declined it on the Ledger. Nothing was signed."
        case 0x5515, 0x6b0c, 0x6982: "The Ledger is locked: enter your PIN on it."
        case 0x6d00, 0x6e00, 0x6e01, 0x6511, 0x6a82: "Open the \(app) app on your Ledger."
        case 0x6807: "The \(app) app isn't installed on this Ledger: add it in Ledger Live (My Ledger)."
        case 0x6a80 where app == "Ethereum": "The Ethereum app refused the data: turn on «Blind signing» in its settings on the Ledger — contract calls and Hyperliquid orders need it."
        case 0x6808 where app == "Solana": "The Solana app can't show this transaction in full: turn on «Blind signing» in its settings on the Ledger — USDC transfers, swaps and lending need it for now."
        default: "The Ledger answered 0x\(String(status, radix: 16)) in the \(app) app."
        }
        return LedgerError(text, status: status)
    }
}

/// Ledger's framing: tag 0x05, a sequence number, and on the first packet the APDU's length. Over USB (HID) each
/// packet starts with channel 0x0101 and is padded to 64 bytes; over Bluetooth there is neither, and a packet is at
/// most the link's MTU.
public enum LedgerFraming {
    public static let hidPacketSize = 64

    public static func packets(_ apdu: Data, size: Int = hidPacketSize, hid: Bool = true) -> [Data] {
        var rest = Data([UInt8(apdu.count >> 8), UInt8(apdu.count & 0xff)]) + apdu
        var out: [Data] = []
        repeat {
            var p = (hid ? Data([0x01, 0x01]) : Data()) + Data([0x05, UInt8(out.count >> 8), UInt8(out.count & 0xff)])
            let take = min(size - p.count, rest.count)
            p += rest.prefix(take)
            rest = Data(rest.dropFirst(take))
            if hid { p += Data(count: size - p.count) }
            out.append(p)
        } while !rest.isEmpty
        return out
    }

    /// The reply once every packet of it has arrived; nil while more are due.
    public static func reply(_ packets: [Data], hid: Bool = true) throws -> Data? {
        var body = Data()
        let head = hid ? 5 : 3
        for (i, p) in packets.enumerated() {
            let b = [UInt8](p)
            guard b.count >= head, !hid || (b[0] == 1 && b[1] == 1), b[head - 3] == 5, Int(b[head - 2]) << 8 | Int(b[head - 1]) == i else {
                throw LedgerError("The Ledger sent a packet out of order.")
            }
            body += b.dropFirst(head)
        }
        guard body.count >= 2 else { return nil }
        let length = Int(body[body.startIndex]) << 8 | Int(body[body.startIndex + 1])
        guard body.count - 2 >= length else { return nil }
        return Data(body.dropFirst(2).prefix(length))
    }
}

/// One Ledger and the apps on it. Every call names the app it needs; the caller opens it first (`open`).
public struct LedgerDevice: Sendable {
    public let transport: any LedgerTransport
    public init(_ transport: any LedgerTransport) { self.transport = transport }

    /// Seconds the person has to read and approve on the device.
    static let confirmTimeout: TimeInterval = 180

    func send(_ cla: UInt8, _ ins: UInt8, _ p1: UInt8 = 0, _ p2: UInt8 = 0, _ data: Data = Data(), app: String,
              timeout: TimeInterval = 10) async throws -> Data {
        precondition(data.count <= 255)
        let reply = try await transport.exchange(Data([cla, ins, p1, p2, UInt8(data.count)]) + data, timeout: timeout)
        guard reply.count >= 2 else { throw LedgerError("The Ledger sent an empty reply.") }
        let status = UInt16(reply[reply.endIndex - 2]) << 8 | UInt16(reply[reply.endIndex - 1])
        guard status == 0x9000 else { throw LedgerError.from(status, app: app) }
        return Data(reply.dropLast(2))
    }

    /// BIP-32 path as the apps take it: the number of steps, then each as a big-endian 32-bit index.
    static func path(_ path: String) throws -> Data {
        let steps = try Derivation.parse(path)
        return steps.reduce(Data([UInt8(steps.count)])) { $0 + withUnsafeBytes(of: $1.bigEndian) { Data($0) } }
    }

    // MARK: Dashboard

    /// The app open now and its version; «BOLOS» is the dashboard.
    public func openApp() async throws -> (name: String, version: String) {
        let r = [UInt8](try await send(0xb0, 0x01, app: "dashboard"))
        guard r.count >= 3, r[0] == 1 else { throw LedgerError("The Ledger didn't say which app is open.") }
        let n = Int(r[1])
        guard r.count >= 3 + n else { throw LedgerError("The Ledger didn't say which app is open.") }
        let name = String(decoding: r[2..<(2 + n)], as: UTF8.self)
        let v = Int(r[2 + n])
        let version = r.count >= 3 + n + v ? String(decoding: r[(3 + n)..<(3 + n + v)], as: UTF8.self) : ""
        return (name, version)
    }

    /// Opens `app` («Ethereum», «Solana»), quitting whatever app is open first. The device may ask to allow it.
    /// The Ledger reconnects over USB after a switch, so the next call finds it anew.
    public func open(_ app: String) async throws {
        var current = try await openApp().name
        guard current != app else { return }
        if current != "BOLOS" {
            _ = try await send(0xb0, 0xa7, app: current)
            for _ in 0..<20 {
                try await Task.sleep(for: .milliseconds(300))
                if let now = try? await openApp().name, now == "BOLOS" { current = now; break }
            }
        }
        _ = try await send(0xe0, 0xd8, 0, 0, Data(app.utf8), app: app, timeout: Self.confirmTimeout)
        for _ in 0..<20 {
            try await Task.sleep(for: .milliseconds(300))
            if let now = try? await openApp().name, now == app { return }
        }
        throw LedgerError("The \(app) app didn't open on the Ledger: open it by hand and try again.")
    }

    // MARK: Ethereum app

    public static let evmPath = "m/44'/60'/0'/0/0"

    /// The EIP-55 address at `path`; `show`: the Ledger displays it for the person to compare, and waits for their OK.
    public func evmAddress(_ path: String = evmPath, show: Bool = false) async throws -> String {
        let r = [UInt8](try await send(0xe0, 0x02, show ? 1 : 0, 0, try Self.path(path), app: "Ethereum",
                                       timeout: show ? Self.confirmTimeout : 10))
        guard let keyLength = r.first, r.count > Int(keyLength) + 1 else { throw LedgerError("The Ethereum app sent no address.") }
        let addressLength = Int(r[Int(keyLength) + 1])
        let start = Int(keyLength) + 2
        guard r.count >= start + addressLength, addressLength == 40 else { throw LedgerError("The Ethereum app sent no address.") }
        return EVMAccount.checksum(String(decoding: r[start..<(start + addressLength)], as: UTF8.self))
    }

    /// Whether «Blind signing» is on in the Ethereum app: contract calls and typed-data hashes need it.
    public func evmBlindSigning() async throws -> (on: Bool, version: String) {
        let r = [UInt8](try await send(0xe0, 0x06, app: "Ethereum"))
        guard r.count >= 4 else { throw LedgerError("The Ethereum app sent no settings.") }
        return (r[0] & 0x01 != 0, "\(r[1]).\(r[2]).\(r[3])")
    }

    /// Signs an unsigned EIP-2718 transaction (0x02 ‖ rlp) after the person reviews it on the device.
    /// Returns r, s and the raw first byte (the y parity for a typed transaction).
    public func signEVMTransaction(_ payload: Data, path: String = evmPath) async throws -> (v: UInt8, r: Data, s: Data) {
        // 150-byte pieces, as Ledger's own libraries send them; the first one starts with the path.
        let head = try Self.path(path)
        var rest = payload, first = true
        var reply = Data()
        while !rest.isEmpty || first {
            let room = first ? 150 - head.count : 150
            let piece = rest.prefix(room)
            rest = Data(rest.dropFirst(piece.count))
            reply = try await send(0xe0, 0x04, first ? 0x00 : 0x80, 0, (first ? head : Data()) + piece, app: "Ethereum",
                                   timeout: rest.isEmpty ? Self.confirmTimeout : 10)
            first = false
        }
        guard reply.count >= 65 else { throw LedgerError("The Ethereum app sent no signature.") }
        return (reply[reply.startIndex], Data(reply.dropFirst().prefix(32)), Data(reply.dropFirst(33).prefix(32)))
    }

    /// Ledger's signed token descriptor (`LedgerDescriptors.erc20`), so the next transaction's ERC-20 transfer or
    /// approval shows its amount and ticker instead of asking for blind signing.
    public func provideERC20(_ payload: Data) async throws {
        _ = try await send(0xe0, 0x0a, 0, 0, payload, app: "Ethereum")
    }

    /// EIP-712 by its two hashes: the device shows them and signs keccak(0x1901 ‖ domain ‖ message).
    /// Needs «Blind signing»: the device can't show what the hashes stand for.
    public func signTypedDataHashes(_ parts: EIP712.Parts, path: String = evmPath) async throws -> EVMSignature {
        let r = try await send(0xe0, 0x0c, 0, 0, try Self.path(path) + parts.domain + parts.message, app: "Ethereum", timeout: Self.confirmTimeout)
        guard r.count >= 65 else { throw LedgerError("The Ethereum app sent no signature.") }
        let v = Int(r[r.startIndex])
        return EVMSignature(r: Data(r.dropFirst().prefix(32)), s: Data(r.dropFirst(33).prefix(32)), v: v < 27 ? v + 27 : v)
    }

    /// EIP-712 in full: the Ledger gets the types and every value, hashes them itself and shows each field by name
    /// («agentAddress», «destination», «amount») before it signs. The app may show a field as it arrives and wait for
    /// the person, so every step may take as long as a signature.
    public func signTypedData(_ typed: EIP712.Typed, path: String = evmPath) async throws -> EVMSignature {
        for (name, fields) in [("EIP712Domain", typed.domain), (typed.type, typed.message)] {
            _ = try await send(0xe0, 0x1a, 0, 0x00, Data(name.utf8), app: "Ethereum")
            for f in fields {
                _ = try await send(0xe0, 0x1a, 0, 0xff, try Self.fieldType(f.type) + Data([UInt8(f.name.utf8.count)]) + Data(f.name.utf8), app: "Ethereum")
            }
        }
        for (name, fields) in [("EIP712Domain", typed.domain), (typed.type, typed.message)] {
            _ = try await send(0xe0, 0x1c, 0, 0x00, Data(name.utf8), app: "Ethereum")
            for f in fields {
                let value = try Self.fieldValue(f.value)
                var rest = Data([UInt8(value.count >> 8), UInt8(value.count & 0xff)]) + value
                while !rest.isEmpty {
                    let piece = Data(rest.prefix(255))
                    rest = Data(rest.dropFirst(piece.count))
                    _ = try await send(0xe0, 0x1c, rest.isEmpty ? 0 : 1, 0xff, piece, app: "Ethereum", timeout: Self.confirmTimeout)
                }
            }
        }
        let r = try await send(0xe0, 0x0c, 0, 0x01, try Self.path(path), app: "Ethereum", timeout: Self.confirmTimeout)
        guard r.count >= 65 else { throw LedgerError("The Ethereum app sent no signature.") }
        let v = Int(r[r.startIndex])
        return EVMSignature(r: Data(r.dropFirst().prefix(32)), s: Data(r.dropFirst(33).prefix(32)), v: v < 27 ? v + 27 : v)
    }

    /// A field's type as the Ethereum app reads it: a kind in the low bits, 0x40 when a byte size follows.
    static func fieldType(_ type: String) throws -> Data {
        switch type {
        case "string": return Data([0x05])
        case "address": return Data([0x03])
        case "bool": return Data([0x04])
        case "bytes": return Data([0x07])
        default:
            for (prefix, kind) in [("uint", UInt8(2)), ("int", 1), ("bytes", 6)] where type.hasPrefix(prefix) {
                guard let n = Int(type.dropFirst(prefix.count)) else { break }
                return Data([0x40 | kind, UInt8(prefix == "bytes" ? n : n / 8)])
            }
            throw LedgerError("The Ledger can't be sent a field of type \(type).")
        }
    }

    /// A value as raw bytes: an integer in as few bytes as it needs, an address in 20.
    static func fieldValue(_ value: EIP712.Value) throws -> Data {
        switch value {
        case .string(let s): return Data(s.utf8)
        case .address(let a):
            guard let bytes = Data(hex: a), bytes.count == 20 else { throw LedgerError("Invalid address: \(a)") }
            return bytes
        case .uint(let n):
            let be = withUnsafeBytes(of: n.bigEndian) { Data($0) }
            return Data(be.drop { $0 == 0 }).isEmpty ? Data([0]) : Data(be.drop { $0 == 0 })
        case .bool(let b): return Data([b ? 1 : 0])
        case .bytes32(let d), .bytes(let d): return d
        }
    }

    // MARK: Solana app

    /// Ledger Live's first Solana account.
    public static let solanaPath = "m/44'/501'/0'"

    public func solanaAddress(_ path: String = solanaPath, show: Bool = false) async throws -> String {
        let r = try await send(0xe0, 0x05, show ? 1 : 0, 0, try Self.path(path), app: "Solana", timeout: show ? Self.confirmTimeout : 10)
        guard r.count == 32 else { throw LedgerError("The Solana app sent no address.") }
        return Base58.encode(r)
    }

    /// A certificate Ledger signed with its root (`LedgerDescriptors.trustedNameCertificate`): the key the next name
    /// descriptors are checked with. An OS command, answered by whichever app is open.
    public func loadCertificate(_ payload: Data, usage: UInt8 = 0x04, app: String) async throws {
        _ = try await send(0xb0, 0x06, usage, 0, payload, app: app)
    }

    /// Four fresh bytes the next trusted-name descriptor must carry, so an old one can't be replayed.
    public func challenge(app: String) async throws -> Data {
        let r = try await send(0xe0, 0x20, app: app)
        guard r.count == 4 else { throw LedgerError("The \(app) app sent no challenge.") }
        return r
    }

    /// «This token account is that owner's», signed by Ledger for the challenge: the Solana app then names the recipient.
    public func provideTrustedName(_ tlv: Data, app: String) async throws {
        _ = try await send(0xe0, 0x21, 0, 0, tlv, app: app)
    }

    public func solanaBlindSigning() async throws -> (on: Bool, version: String) {
        let r = [UInt8](try await send(0xe0, 0x04, app: "Solana"))
        guard r.count >= 5 else { throw LedgerError("The Solana app sent no settings.") }
        return (r[0] != 0, "\(r[2]).\(r[3]).\(r[4])")
    }

    /// Signs a Solana transaction message (what follows the signatures in the wire) after the person reviews it.
    public func signSolana(_ message: Data, path: String = solanaPath) async throws -> Data {
        // One signer, its path, then the message; in 255-byte pieces marked «more» / «extends the last one».
        let payload = Data([1]) + (try Self.path(path)) + message
        var offset = 0, p2: UInt8 = 0
        while payload.count - offset > 255 {
            _ = try await send(0xe0, 0x06, 1, p2 | 0x02, Data(payload[offset..<(offset + 255)]), app: "Solana")
            offset += 255
            p2 |= 0x01
        }
        let r = try await send(0xe0, 0x06, 1, p2, Data(payload[offset...]), app: "Solana", timeout: Self.confirmTimeout)
        guard r.count == 64 else { throw LedgerError("The Solana app sent no signature.") }
        return r
    }
}

/// Speculos, Ledger's emulator, over its APDU socket: a 4-byte big-endian length and the APDU; the reply is a 4-byte
/// length, the data and the status word. For tests; the app talks to a real device over USB.
public struct LedgerTCPTransport: LedgerTransport {
    public let host: String
    public let port: UInt16
    public init(host: String = "127.0.0.1", port: UInt16 = 9999) { (self.host, self.port) = (host, port) }

    public func exchange(_ apdu: Data, timeout: TimeInterval) async throws -> Data {
        let (host, port) = (host, port)
        return try await withCheckedThrowingContinuation { c in
            DispatchQueue.global().async {
                c.resume(with: Result { try Self.exchangeSync(apdu, host: host, port: port, timeout: timeout) })
            }
        }
    }

    static func exchangeSync(_ apdu: Data, host: String, port: UInt16, timeout: TimeInterval) throws -> Data {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw LedgerError("No socket for the emulator.") }
        defer { close(fd) }
        var tv = timeval(tv_sec: Int(timeout), tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))
        var addr = sockaddr_in(sin_len: UInt8(MemoryLayout<sockaddr_in>.size), sin_family: sa_family_t(AF_INET), sin_port: port.bigEndian,
                               sin_addr: in_addr(s_addr: inet_addr(host)), sin_zero: (0, 0, 0, 0, 0, 0, 0, 0))
        let connected = withUnsafePointer(to: &addr) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) } }
        guard connected == 0 else { throw LedgerError("The Ledger emulator isn't running on \(host):\(port).") }
        let out = withUnsafeBytes(of: UInt32(apdu.count).bigEndian) { Data($0) } + apdu
        guard out.withUnsafeBytes({ write(fd, $0.baseAddress, out.count) }) == out.count else { throw LedgerError("The emulator closed the connection.") }
        func read(_ n: Int) throws -> Data {
            var got = Data()
            var buffer = [UInt8](repeating: 0, count: max(n, 1))
            while got.count < n {
                let k = Darwin.read(fd, &buffer, n - got.count)
                guard k > 0 else { throw LedgerError("No answer from the emulator.") }
                got += buffer.prefix(k)
            }
            return got
        }
        let length = try read(4).reduce(0) { $0 << 8 | Int($1) }
        return try read(length + 2)
    }
}

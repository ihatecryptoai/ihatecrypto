import CWinSec
import Foundation
import SignerAPI
import WalletCore

/// The engine's side of the signing process: it starts `ihatecrypto-signer` from its own folder with pipes, hardened from
/// birth (winsec_spawn_signer), and sends it one request at a time — the same SignerRequest and SignerReply as the
/// Mac's XPC. A signer that died is started again on the next request.
actor SignerLink {
    static let shared = SignerLink()
    /// Told when the signer stopped answering (crashed, or killed by another program): the window keeps it with its crash
    /// reports, as the Mac collects the signer's (Crashes.swift).
    nonisolated(unsafe) static var stopped: (@Sendable (String) -> Void)?

    private var child: (pid: UInt32, process: Int, input: Int, output: Int)?
    /// The signer's process id, for the window to let it come forward when Windows Hello asks (AllowSetForegroundWindow).
    private(set) var pid: UInt32?
    /// One exchange on the pipes at a time: the actor gives way while a reply is awaited (a Windows Hello prompt can take
    /// a minute), and a second request — a lock's endSession, a status read — would write and read the same pipes.
    private var busy = false
    private var queue: [CheckedContinuation<Void, Never>] = []

    private func take() async {
        guard busy else { busy = true; return }
        await withCheckedContinuation { queue.append($0) }
    }

    private func give() {
        if queue.isEmpty { busy = false } else { queue.removeFirst().resume() }
    }

    static var executable: URL {
        let me = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
        #if os(Windows)
        return me.appendingPathComponent("ihatecrypto-signer.exe")
        #else
        return me.appendingPathComponent("ihatecrypto-signer")
        #endif
    }

    static var isAvailable: Bool { FileManager.default.fileExists(atPath: executable.path) }

    private func start() throws -> (pid: UInt32, process: Int, input: Int, output: Int) {
        if let child { return child }
        // The system's own form of the path («C:\\Program Files\\…»): URL.path can read «/C:/…» on Windows.
        let path = Self.executable.withUnsafeFileSystemRepresentation { $0.map { Array(String(cString: $0).utf8) } } ?? Array(Self.executable.path.utf8)
        var pid: UInt32 = 0
        var process = 0, input = 0, output = 0
        guard winsec_spawn_signer(path, path.count, &pid, &process, &input, &output) == WINSEC_OK else {
            throw WalletError("The signer didn't start: \(Self.executable.path) is missing or Windows refused it.")
        }
        let started = (pid, process, input, output)
        child = started
        self.pid = pid
        return started
    }

    private func drop() {
        guard let c = child else { return }
        winsec_close(c.input)
        winsec_close(c.output)
        winsec_close(c.process)
        child = nil
        pid = nil
    }

    func send(_ request: SignerRequest) async throws -> SignerReply {
        let body = try JSONEncoder().encode(request)
        await take()
        defer { give() }
        // A dead signer (killed, crashed) is started once more for this request.
        for attempt in 0..<2 {
            let c = try start()
            // The pipes block: off the actor's thread, so a Windows Hello prompt doesn't hold the engine up.
            let answer: Data? = await withCheckedContinuation { done in
                Thread.detachNewThread {
                    done.resume(returning: Self.exchange(body, input: c.input, output: c.output))
                }
            }
            if let answer { return try JSONDecoder().decode(SignerReply.self, from: answer) }
            // Only the request's name: its amounts, addresses and bytes never go into a crash report.
            let name = Mirror(reflecting: request).children.first?.label ?? String(describing: request)
            Self.stopped?("the signer stopped answering on «\(name)»; it was started again")
            drop()
            if attempt == 1 { break }
        }
        throw WalletError("The signer stopped answering: nothing was signed.")
    }

    /// One frame out, one back: a 4-byte little-endian length, then the JSON.
    private static func exchange(_ body: Data, input: Int, output: Int) -> Data? {
        let n = body.count
        let frame = [UInt8(n & 0xff), UInt8(n >> 8 & 0xff), UInt8(n >> 16 & 0xff), UInt8(n >> 24 & 0xff)] + [UInt8](body)
        guard winsec_write(input, frame, frame.count) == WINSEC_OK else { return nil }
        var header = [UInt8](repeating: 0, count: 4)
        guard winsec_read(output, &header, 4) == WINSEC_OK else { return nil }
        let length = Int(header[0]) | Int(header[1]) << 8 | Int(header[2]) << 16 | Int(header[3]) << 24
        guard length <= 16 << 20 else { return nil }
        var reply = [UInt8](repeating: 0, count: length)
        guard length == 0 || winsec_read(output, &reply, length) == WINSEC_OK else { return nil }
        return Data(reply)
    }
}

import CWinSec
import Foundation
import SignerCore

// The signing process on Windows. Started by the engine (ihatecrypto-engine.exe, next to it) with pipes on its standard
// input and output; it takes requests only from there. Frames: a 4-byte little-endian length, then that many bytes of
// JSON (SignerRequest in, SignerReply out).
let failed = winsec_harden()
SignerHost.notes = SignerHost.describe(hardening: failed)

let engine = Array("ihatecrypto-engine.exe".utf8)
guard winsec_parent_check(engine, engine.count) == WINSEC_OK else {
    FileHandle.standardError.write(Data("ihatecrypto signer: started by something other than ihatecrypto's engine; nothing is served.\n".utf8))
    exit(3)
}

var input = 0, output = 0
guard winsec_stdio(&input, &output) == WINSEC_OK else { exit(4) }

func readFrame(_ input: Int) -> Data? {
    var header = [UInt8](repeating: 0, count: 4)
    guard winsec_read(input, &header, 4) == WINSEC_OK else { return nil }
    let length = Int(header[0]) | Int(header[1]) << 8 | Int(header[2]) << 16 | Int(header[3]) << 24
    guard length <= 16 << 20 else { return nil }
    var body = [UInt8](repeating: 0, count: length)
    guard length == 0 || winsec_read(input, &body, length) == WINSEC_OK else { return nil }
    return Data(body)
}

func writeFrame(_ output: Int, _ data: Data) -> Bool {
    let n = data.count
    let frame = [UInt8(n & 0xff), UInt8(n >> 8 & 0xff), UInt8(n >> 16 & 0xff), UInt8(n >> 24 & 0xff)] + [UInt8](data)
    return winsec_write(output, frame, frame.count) == WINSEC_OK
}

// One request at a time, as the Mac's XPC connection delivers them; the engine's pipe closing ends the signer.
while let request = readFrame(input) {
    guard writeFrame(output, SignerHost.handle(request)) else { break }
}

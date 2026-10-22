import Foundation

// The engine of the desktop app, started by its window (Tauri) with pipes on standard input and output: one JSON
// request a line in (Engine.Request), one JSON answer or event a line out (Outbox). The window closing its end ends it.
let engine = Engine(out: Outbox())

Thread.detachNewThread {
    let decoder = JSONDecoder()
    while let line = readLine(strippingNewline: true) {
        guard let data = line.data(using: .utf8), let request = try? decoder.decode(Engine.Request.self, from: data) else { continue }
        Task { @MainActor in await engine.handle(request) }
    }
    exit(0)
}

dispatchMain()

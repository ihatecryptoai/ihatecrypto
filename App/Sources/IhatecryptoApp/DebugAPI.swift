import AppKit
import EVMChain
import GuardJev
import IhatecryptoCore
import Network
import SignerAPI
import SwiftUI

// Only in the copies build-app.sh makes (DEBUG_API, project.yml): a release has no debug API, no Settings tab for
// it and no badge.
#if DEBUG_API
/// Shown next to the zoom while the local debug API is on, so it is never left on unnoticed.
struct DebugBadge: View {
    @AppStorage("debugAPI") private var on = false

    var body: some View {
        if on {
            Label("Debug API", systemImage: "ladybug.fill").font(.ui(11, .semibold)).foregroundStyle(.orange)
                .help("The local debug API is on: Settings → Debugging")
        }
    }
}

/// A local door for driving ihatecrypto from a terminal, e.g. Claude Code with curl:
///
///     T=$(cat ~/Library/Application\ Support/ihatecrypto/debug-token)
///     curl -s -H "Authorization: Bearer $T" 127.0.0.1:51789/state
///     curl -s -H "Authorization: Bearer $T" -d '{"text":"выведи всё из Aave"}' 127.0.0.1:51789/send
///     curl -s -H "Authorization: Bearer $T" 127.0.0.1:51789/screenshot -o ihatecrypto.png
///     curl -s -H "Authorization: Bearer $T" -X POST 127.0.0.1:51789/reset      # a fresh canvas for a test
///     curl -s -H "Authorization: Bearer $T" -X POST 127.0.0.1:51789/cancel     # drop what waits for Confirm
///     curl -s -H "Authorization: Bearer $T" -d '{"index":0}' 127.0.0.1:51789/pick   # press a «What next» card
///     curl -s -H "Authorization: Bearer $T" -d '{"canvas":"<id>"}' 127.0.0.1:51789/restore   # delete test canvases, reopen
///     curl -s -H "Authorization: Bearer $T" -d '{"factor":2}' 127.0.0.1:51789/zoom   # zoom in ({"fit":true}: 100 %)
///     curl -s -H "Authorization: Bearer $T" -d '{"seconds":4}' 127.0.0.1:51789/pan    # pan back and forth, for xctrace
///
/// Tools/uitest.py runs scenarios through it (Tools/uitest-scenarios.json).
///
/// Off unless Settings turns it on; 127.0.0.1 only; a token that changes every time it starts, in an owner-only
/// file; anything a browser sends (an Origin header, another Host) is refused. It reads the state, types into the
/// chat and takes a picture of the window. It never signs: while its message is handled only status requests reach
/// the signer (SignerClient.untrusted), so money moves still wait for Confirm and Touch ID in the window.
@MainActor
final class DebugAPI {
    static let shared = DebugAPI()
    /// The development copy answers on the next port, so both copies' APIs can be on at once.
    static let port: UInt16 = Flavor.metal ? 51791 : Flavor.dev ? 51790 : 51789
    private var listener: NWListener?
    private var token = ""
    private weak var ws: Workspace?
    /// Canvases a test made through /reset and /send: /restore deletes them, so tests leave no trace in the list.
    /// Kept across launches: another session may relaunch the app between a test and its /restore.
    private var testCanvases = Set((UserDefaults.standard.stringArray(forKey: "debugTestCanvases") ?? []).compactMap(UUID.init(uuidString:))) {
        didSet { UserDefaults.standard.set(testCanvases.map(\.uuidString), forKey: "debugTestCanvases") }
    }
    /// /reset opened a fresh canvas: the first /send's canvas is a test canvas.
    private var freshCanvas = false
    /// Folders a scenario made («write to folder …»): /restore removes them with the test canvases.
    private var testFolders: Set<String> = []
    /// Scheduled tasks a test message made («every month on the 1st: …»): /reset and /restore cancel them, or they'd
    /// run for real on their day. Kept across launches, like the canvases.
    private var testTasks = Set((UserDefaults.standard.stringArray(forKey: "debugTestTasks") ?? []).compactMap(UUID.init(uuidString:))) {
        didSet { UserDefaults.standard.set(testTasks.map(\.uuidString), forKey: "debugTestTasks") }
    }

    func start(_ ws: Workspace) {
        guard listener == nil, let port = NWEndpoint.Port(rawValue: Self.port) else { return }
        self.ws = ws
        token = (0..<24).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
        let file = LocalStore.url("debug-token")
        try? FileManager.default.removeItem(at: file)
        FileManager.default.createFile(atPath: file.path, contents: Data(token.utf8), attributes: [.posixPermissions: 0o600])
        // The loopback interface only: nothing from the network can reach it.
        let parameters = NWParameters.tcp
        parameters.requiredInterfaceType = .loopback
        parameters.allowLocalEndpointReuse = true
        guard let listener = try? NWListener(using: parameters, on: port) else { return }
        listener.stateUpdateHandler = { state in
            if case .failed(let error) = state { FileHandle.standardError.write(Data("ihatecrypto debug API: \(error)\n".utf8)) }
        }
        listener.newConnectionHandler = { connection in
            connection.start(queue: .main)
            Self.read(connection, Data())
        }
        listener.start(queue: .main)
        self.listener = listener
    }

    func stop() {
        listener?.cancel()
        listener = nil
        try? FileManager.default.removeItem(at: LocalStore.url("debug-token"))
    }

    private nonisolated static func read(_ connection: NWConnection, _ buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 1 << 16) { data, _, done, error in
            let all = buffer + (data ?? Data())
            if let request = Request(all) {
                Task { @MainActor in
                    let (status, type, body) = await DebugAPI.shared.handle(request)
                    let head = "HTTP/1.1 \(status) \(status == 200 ? "OK" : "Error")\r\nContent-Type: \(type)\r\nContent-Length: \(body.count)\r\nConnection: close\r\n\r\n"
                    connection.send(content: Data(head.utf8) + body, completion: .contentProcessed { _ in connection.cancel() })
                }
            } else if done || error != nil || all.count > 1 << 20 {
                connection.cancel()
            } else {
                read(connection, all)
            }
        }
    }

    private func handle(_ r: Request) async -> (Int, String, Data) {
        // A page in a browser always sends Origin, and a name pointed at 127.0.0.1 shows in Host: neither gets in.
        let hosts = ["127.0.0.1:\(Self.port)", "localhost:\(Self.port)"]
        guard r.headers["origin"] == nil, hosts.contains(r.headers["host"] ?? ""), r.headers["authorization"] == "Bearer \(token)" else {
            return (403, "text/plain", Data("forbidden\n".utf8))
        }
        guard let ws else { return (503, "text/plain", Data("no workspace\n".utf8)) }
        // Locked: the window shows nothing, and neither does this.
        if AppLock.shared.locked {
            return r.path == "/state" ? (200, "application/json", Data(#"{"locked":true}"#.utf8)) : (423, "text/plain", Data("locked\n".utf8))
        }
        switch (r.method, r.path) {
        case ("GET", "/state"):
            return (200, "application/json", json(state(ws, since: max(0, ws.messages.count - 12))))
        case ("POST", "/send"):
            let text = ((try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any])?["text"] as? String
                ?? String(decoding: r.body, as: UTF8.self)
            guard !text.isEmpty else { return (400, "text/plain", Data("no text\n".utf8)) }
            guard !ws.busy else { return (409, "text/plain", Data("busy\n".utf8)) }
            let before = ws.messages.count
            let knownCanvases = Set(ws.canvases.map(\.id)), knownFolders = Set(ws.folders), knownTasks = Set(ws.tasks.map(\.id))
            await SignerClient.$untrusted.withValue(true) { await ws.send(text) }
            testTasks.formUnion(ws.tasks.map(\.id).filter { !knownTasks.contains($0) })
            if freshCanvas, let id = ws.canvasID { testCanvases.insert(id); freshCanvas = false }
            // A note canvas or a folder the message made goes too, so a scenario leaves nothing in the sidebar.
            testCanvases.formUnion(ws.canvases.map(\.id).filter { !knownCanvases.contains($0) })
            testFolders.formUnion(ws.folders.filter { !knownFolders.contains($0) })
            return (200, "application/json", json(state(ws, since: before)))
        case ("POST", "/reset"):
            // A card waiting on the user's own canvas is never pushed aside by a test.
            guard !ws.busy, !ws.executing else { return (409, "text/plain", Data("busy\n".utf8)) }
            let testCanvas = ws.canvasID.map { testCanvases.contains($0) } ?? freshCanvas
            guard testCanvas || (ws.pendingAction == nil && ws.pendingAllocation == nil) else {
                return (409, "text/plain", Data("a move waits for Confirm on the user's canvas\n".utf8))
            }
            if let id = ws.canvasID, testCanvases.contains(id) { ws.deleteCanvas(id) } else { ws.newCanvas() }
            freshCanvas = true
            dropTestTasks(ws)
            return (200, "application/json", json(state(ws, since: ws.messages.count)))
        case ("POST", "/pick"):
            let index = (((try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any])?["index"] as? Int) ?? 0
            guard !ws.busy else { return (409, "text/plain", Data("busy\n".utf8)) }
            guard let options = ws.liveSteps?.options, options.indices.contains(index) else { return (404, "text/plain", Data("no such card\n".utf8)) }
            let before = ws.messages.count, knownTasks = Set(ws.tasks.map(\.id))
            await SignerClient.$untrusted.withValue(true) { await ws.pick(options[index]) }
            testTasks.formUnion(ws.tasks.map(\.id).filter { !knownTasks.contains($0) })
            return (200, "application/json", json(state(ws, since: before)))
        case ("GET", "/context"):
            // What the model reads besides the message: balances, canvas blocks, the chat, memory.
            let c = ws.debugContext
            return (200, "application/json", json(["balances": c.balances, "canvas": c.canvas, "recent": c.recent, "memory": c.memory,
                                                   "favorites": c.favorites, "positions": c.positions, "off": c.off]))
        case ("POST", "/stop"):
            // The Stop button: cuts the model off mid-answer (never a signed move).
            let running = ws.modelRunning
            ws.stopModel()
            return (200, "application/json", json(["stopped": running && !ws.executing]))
        case ("POST", "/cancel"):
            guard !ws.busy else { return (409, "text/plain", Data("busy\n".utf8)) }
            let before = ws.messages.count
            if ws.pendingAction != nil || ws.pendingAllocation != nil || ws.awaitingAmount != nil {
                await SignerClient.$untrusted.withValue(true) { await ws.send("cancel") }
            }
            return (200, "application/json", json(state(ws, since: before)))
        case ("POST", "/restore"):
            guard !ws.busy, !ws.executing else { return (409, "text/plain", Data("busy\n".utf8)) }
            let back = (((try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any])?["canvas"] as? String).flatMap(UUID.init(uuidString:))
            if let id = ws.canvasID, testCanvases.contains(id) { ws.newCanvas() }
            for id in testCanvases { ws.deleteCanvas(id) }
            testCanvases = []
            for f in testFolders { ws.deleteFolder(f) }
            testFolders = []
            dropTestTasks(ws)
            freshCanvas = false
            if let back, let canvas = ws.canvases.first(where: { $0.id == back }) { ws.openCanvas(canvas) }
            return (200, "application/json", json(state(ws, since: ws.messages.count)))
        case ("POST", "/zoom"):
            // Zooms the canvas like ⌘-scroll (factor 2 = twice as close), or back to 100 % with {"fit":true}.
            let body = (try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any]
            if body?["fit"] as? Bool == true { NotificationCenter.default.post(name: .canvasFit, object: nil) }
            else {
                NotificationCenter.default.post(name: .canvasZoom, object: nil,
                                                userInfo: ["factor": CGFloat(body?["factor"] as? Double ?? 1), "hold": body?["hold"] as? Bool ?? false])
            }
            return (200, "text/plain", Data("ok\n".utf8))
        case ("POST", "/pan"):
            // A drag without a hand for measuring: `seconds` of panning at 120 steps a second, back and forth by `dx`.
            let body = (try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any]
            let seconds = min(max(body?["seconds"] as? Double ?? 3, 0), 60), dx = body?["dx"] as? Double ?? 300
            let steps = Int(seconds * 120)
            for i in 0..<steps {
                let dir: CGFloat = (i / 60) % 2 == 0 ? 1 : -1
                NotificationCenter.default.post(name: .canvasPan, object: nil, userInfo: ["dx": dir * CGFloat(dx) / 60, "dy": dir * CGFloat(dx) / 120])
                try? await Task.sleep(for: .milliseconds(8))
            }
            return (200, "text/plain", Data("ok\n".utf8))
        case ("GET", "/layers"):
            // The window's layer tree by class: how many, and the megabytes their bitmaps take at their contentsScale.
            guard let root = NSApp.windows.first(where: { $0.isVisible && $0.title == AppMain.mainTitle })?.contentView?.layer else {
                return (409, "text/plain", Data("no window\n".utf8))
            }
            var byClass: [String: (count: Int, mb: Double, scale: Double)] = [:]
            func walk(_ layer: CALayer) {
                let name = NSStringFromClass(type(of: layer))
                let s = layer.contentsScale, mb = layer.contents == nil ? 0 : layer.bounds.width * s * layer.bounds.height * s * 4 / 1_048_576
                let old = byClass[name] ?? (0, 0, 0)
                byClass[name] = (old.count + 1, old.mb + mb, max(old.scale, s))
                layer.sublayers?.forEach(walk)
            }
            walk(root)
            let rows = byClass.sorted { $0.value.mb > $1.value.mb }.map { ["class": $0.key, "count": $0.value.count, "mb": ($0.value.mb * 10).rounded() / 10, "maxScale": $0.value.scale] as [String: Any] }
            return (200, "application/json", (try? JSONSerialization.data(withJSONObject: rows, options: [.sortedKeys])) ?? Data())
        case ("POST", "/measure"):
            // Frames while a hand-free scenario runs (FrameMeter; Tools/perf.py runs the set): {"what": "pan"|"pinch"|"zoom"|
            // "idle"|"thinking"|"chat"|"switch"|"newcanvas", "seconds": 3}, plus «classic», «noGlass», «noMeter» and «stay»
            // (the window left in front) below. A pinch goes in and out (≈ 1× → 2× → 1×) in steps of 1.2 %, 120 a second,
            // like a trackpad's magnify events.
            let body = (try? JSONSerialization.jsonObject(with: r.body)) as? [String: Any]
            let what = body?["what"] as? String ?? "pan", seconds = min(max(body?["seconds"] as? Double ?? 3, 0.5), 30)
            guard let window = NSApp.windows.first(where: { $0.isVisible && $0.title == AppMain.mainTitle }), let view = window.contentView else {
                return (409, "text/plain", Data("no window\n".utf8))
            }
            // A covered window isn't drawn, and its numbers would be nothing's: in front for the run, without taking the
            // keyboard, and back behind after (another copy on the same spot covered it).
            window.orderFrontRegardless()
            try? await Task.sleep(for: .milliseconds(300))
            let visible = window.occlusionState.contains(.visible)
            defer { if body?["stay"] as? Bool != true { window.orderBack(nil) } }
            // «noMeter»: the scenario without the display link, which itself keeps the display at 120 Hz.
            let metered = body?["noMeter"] as? Bool != true
            let meter = FrameMeter()
            if metered { meter.start(in: view) }
            // «classic»: the run on the rendering as it was before the metal branch (`Render.classic`), for an A/B in one build.
            Render.classic = body?["classic"] as? Bool ?? false
            Render.shared.noGlass = body?["noGlass"] as? Bool ?? false
            if Render.shared.noGlass { try? await Task.sleep(for: .milliseconds(300)) }
            defer { Render.classic = false; Render.shared.noGlass = false }
            // «thinking»: the chat's face and dots as while a model answers (nothing is asked of a model).
            let wasThinking = ws.thinking
            if what == "thinking" { ws.thinking = true }
            defer { if what == "thinking" { ws.thinking = wasThinking } }
            // «switch»: two canvases (ids in «canvases») open in turn every half second. «chat»: a reply lands every
            // half second (taken out again after). «zoom»: 2× in at once, back out after 1.5 s, sharpening included.
            let pair = ((body?["canvases"] as? [String]) ?? []).compactMap(UUID.init(uuidString:)).compactMap { id in ws.canvases.first { $0.id == id } }
            // «newcanvas»: an empty new canvas open, its row breathing, then the canvas that was open again.
            let before = ws.canvases.first { $0.id == ws.canvasID }
            defer { if what == "newcanvas", let before { ws.openCanvas(before) } }
            var said = 0
            defer { if said > 0 { ws.messages.removeLast(min(said, ws.messages.count)) } }
            let steps = Int(seconds * 120)
            for i in 0..<steps {
                let dir: CGFloat = (i / 60) % 2 == 0 ? 1 : -1
                switch what {
                case "pan": NotificationCenter.default.post(name: .canvasPan, object: nil, userInfo: ["dx": dir * 5, "dy": dir * 2.5])
                case "pinch": NotificationCenter.default.post(name: .canvasZoom, object: nil, userInfo: ["factor": CGFloat(dir > 0 ? 1.012 : 1 / 1.012)])
                case "zoom" where i % 180 == 0: NotificationCenter.default.post(name: .canvasZoom, object: nil, userInfo: ["factor": CGFloat(i % 360 == 0 ? 2 : 0.5)])
                case "switch" where i % 60 == 0 && pair.count == 2: ws.openCanvas(pair[(i / 60) % 2])
                case "newcanvas" where i == 0: ws.newCanvas()
                case "chat" where i % 60 == 30:
                    ws.messages.append(Message(fromUser: false, text: "Measured reply \(i / 60): the plan is on the canvas. Sign it, ask about it, or ask for another limit, e.g. “max 70% per protocol”."))
                    said += 1
                default: break
                }
                try? await Task.sleep(for: .milliseconds(8))
            }
            // The scenario's end lands too (a pan's commit, a pinch's sharp redraw).
            try? await Task.sleep(for: .milliseconds(400))
            var result: [String: Any] = metered ? meter.stop() : [:]
            result["visible"] = visible
            return (200, "application/json", (try? JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])) ?? Data())
        case ("GET", let path) where path.hasPrefix("/render/approval"):
            // The browser's signing sheet for a sample Kamino deposit, drawn off screen («?dark» for the dark look):
            // to see a design change without a site asking for a real signature. Nothing can be approved from it.
            var png: Data?
            _ = await withCheckedContinuation { (done: CheckedContinuation<Bool, Never>) in
                png = Self.renderApproval(SiteRequest(origin: "https://kamino.com", site: .known, kind: .transaction(Self.sampleReview, send: false), answer: done),
                                          ws: ws, dark: path.contains("dark"))
                done.resume(returning: false)
            }
            guard let png else { return (500, "text/plain", Data("couldn't draw\n".utf8)) }
            return (200, "image/png", png)
        case ("GET", "/crashes"):
            // Kept crash reports, newest first: file (in Application Support/ihatecrypto/Crashes), date, summary.
            _ = Crashes.collect()
            return (200, "application/json", json(["crashes": Crashes.all.map { ["file": $0.file, "date": Fmt.date($0.date), "summary": $0.summary] }]))
        case ("GET", "/screenshot"):
            guard let png = screenshot() else { return (500, "text/plain", Data("no window\n".utf8)) }
            return (200, "image/png", png)
        default:
            return (404, "text/plain", Data("GET /state · POST /send {\"text\":…} · GET /screenshot · GET /crashes · POST /reset · POST /cancel · POST /restore {\"canvas\":…}\n".utf8))
        }
    }

    private func dropTestTasks(_ ws: Workspace) {
        guard !testTasks.isEmpty else { return }
        ws.tasks.removeAll { testTasks.contains($0.id) }
        testTasks = []
    }

    private func state(_ ws: Workspace, since: Int) -> [String: Any] {
        let canvas = ws.canvases.first { $0.id == ws.canvasID }
        func num(_ d: Decimal?) -> Any { d.map { NSDecimalNumber(decimal: $0).stringValue } ?? NSNull() }
        return [
            "canvas": canvas?.displayTitle ?? "",
            "canvasID": ws.canvasID?.uuidString ?? NSNull(),
            "executing": ws.executing,
            "model": ws.model,
            "provider": ws.provider.info.name,
            "boards": boards(ws, canvas),
            "outcome": canvas?.outcome.map { $0.ok ? "executed" : "failed" } ?? NSNull(),
            "pending": ws.pendingText ?? NSNull(),
            // Jev's reading of the plan against the chat, or why there is none.
            "jev": ws.planReview.map { ["label": $0.label, "concern": $0.concern, "text": $0.text] as [String: Any] } ?? ws.jevNote ?? NSNull(),
            "checklist": ws.planChecklist,
            // Jev reads the chat (Settings › Second check, with OpenRouter connected): uitest skips what needs it otherwise.
            "secondCheck": ws.jevScreening && ws.jevKey != nil,
            "busy": ws.busy,
            // The chip over the field: tokens the model read with the last message, its window, the answers' dollars.
            "context": ["used": ws.meter.used ?? NSNull(), "window": ws.meter.window ?? NSNull(), "sent": ws.meter.sent, "lines": ws.meter.lines,
                        "spent": ws.meter.spent] as [String: Any],
            "messages": ws.messages.dropFirst(since).map { ["user": $0.fromUser, "text": $0.text, "note": $0.note ?? NSNull(),
                                                             "cards": ($0.options ?? []).map(\.title)] as [String: Any] },
            "balances": [
                "total": num(ws.equity), "solanaUSDC": num(ws.solana?.usdc), "sol": num(ws.solana?.sol),
                "hyperliquid": num(ws.hlTotal), "near": num(ws.near), "kamino": num(ws.kamino), "jupiterLend": num(ws.lendTotal),
                "baseAave": num(ws.base?.aave), "baseMorpho": num(ws.base?.morpho), "baseUSDC": num(ws.base?.usdc),
                "robinhoodUSDG": num(ws.robinhood?.usdg), "arbitrumUSDC": num(ws.arbitrumUSDC),
                "arbitrumUSDT": num(USDT0.on(42161).flatMap { ws.heldUSDC("arbitrum", $0.token) }),
            ] as [String: Any],
            "positions": ws.positions.map { "\($0.size > 0 ? "long" : "short") \($0.coin) pnl \($0.unrealizedPnl)" },
            "topUpPage": TopUpPage.shared.url?.absoluteString ?? NSNull(),
            "wallet": ["solana": ws.solanaAddress, "evm": ws.hlAddress, "near": ws.nearAddress ?? NSNull(), "test": ws.activeWallet?.isTest == true] as [String: Any],
            "favoriteTokens": ws.favoriteTokens.map { "\($0.symbol) \($0.chain) \($0.address)" },
            // For `screencapture -l <id>` from a terminal allowed to record the screen: the glass chrome doesn't
            // render through cacheDisplay (/screenshot comes out blank since the redesign).
            "windowID": NSApp.windows.first { $0.title == AppMain.mainTitle }?.windowNumber ?? 0,
        ]
    }

    /// What the canvas shows, as data a test can assert on: the move waiting for Confirm (its route as drawn), the one
    /// running and its step, finished ones, and the other boards.
    private func boards(_ ws: Workspace, _ canvas: CanvasSession?) -> [String: Any] {
        func steps(_ bubbles: [ActivityBubble]) -> [[String: Any]] {
            bubbles.map { ["label": $0.label, "title": ws.readable($0.title), "lines": $0.lines] as [String: Any] }
        }
        var out: [String: Any] = [:]
        switch ws.pendingAction {
        case .wallet(let command)?:
            let route = ws.route(command)
            let drawn = route ?? ws.fallbackRoute(command)
            out["pending"] = ["kind": "wallet", "command": String(describing: command), "title": drawn.title, "footer": drawn.footer,
                              "steps": steps(drawn.steps), "chain": steps(ws.chainSteps()), "drawnRoute": route != nil, "confirm": true,
                              // What the swap gets as quoted now, the chat's line; nil while quoting or for a move without a price.
                              "quote": ws.swapLine(command) ?? NSNull(), "quoteState": "\(ws.quoteState(at: Date()))"] as [String: Any]
        case .position(let command, let coin)?:
            out["pending"] = ["kind": "position", "command": String(describing: command), "coin": coin, "confirm": true] as [String: Any]
        case .transfer(let entry, let amount)?:
            out["pending"] = ["kind": "transfer", "to": entry.name, "amount": NSDecimalNumber(decimal: amount).stringValue, "confirm": true] as [String: Any]
        case .orders(let commands, let coin, _)?:
            out["pending"] = ["kind": "orders", "commands": commands.map { String(describing: $0) }, "coin": coin, "confirm": true] as [String: Any]
        case .address(let entry)?:
            out["pending"] = ["kind": "address", "name": entry.name, "confirm": true] as [String: Any]
        case nil: break
        }
        if let run = ws.runningAction {
            out["running"] = ["title": run.title, "steps": steps(run.steps), "current": RunningBoard.currentStep(run.steps, run.lines),
                              "lines": run.lines] as [String: Any]
        }
        if let done = ws.finishedAction { out["finished"] = ["title": done.title, "ok": done.outcome.ok] as [String: Any] }
        out["earlier"] = (canvas?.earlier ?? []).map { ["title": ws.readable($0.title), "ok": $0.outcome.ok] as [String: Any] }
        if let y = ws.yieldOptions { out["yields"] = y.map { ["place": $0.place, "asset": $0.asset, "apy": NSDecimalNumber(decimal: $0.apy).doubleValue] as [String: Any] } }
        let pnl = PnLBoardState.shared
        if let command = pnl.command, pnl.canvas == ws.canvasID, let r = pnl.reports[pnl.period] {
            out["pnl"] = ["period": pnl.period.rawValue, "coin": command.coin ?? NSNull(), "trading": NSDecimalNumber(decimal: r.trading).stringValue,
                          "unrealized": NSDecimalNumber(decimal: r.unrealized).stringValue,
                          "change": r.change.map { NSDecimalNumber(decimal: $0).stringValue } ?? NSNull()] as [String: Any]
        }
        if let c = ws.chart {
            out["chart"] = ["coin": c.coin, "range": c.range.rawValue, "candles": c.candles.count, "error": c.error ?? NSNull(),
                            "place": ws.chartPlace(tradePlan: ws.intent != TradeIntent() && !ws.managesPosition)?.rawValue ?? NSNull()] as [String: Any]
        }
        if let d = ws.detail { out["detail"] = d }
        if let n = ws.nextSteps { out["nextSteps"] = n.options.map { ["title": $0.title, "command": $0.command, "recommended": $0.recommended] as [String: Any] } }
        if let a = ws.allocationPreview {
            func n(_ d: Decimal) -> Double { NSDecimalNumber(decimal: d).doubleValue }
            out["allocation"] = ["asset": a.asset, "free": n(a.free), "maxShare": n(a.maxShare), "yearly": n(a.yearly), "leftover": n(a.leftover),
                                 "waits": ws.pendingAllocation != nil,
                                 "slices": a.slices.map { ["place": $0.place, "amount": n($0.amount), "apy": n($0.apy)] as [String: Any] },
                                 "skipped": a.skipped.map { ["place": $0.place, "reason": $0.reason] },
                                 "sources": a.sources.map { ["place": $0.place, "amount": n($0.amount)] as [String: Any] },
                                 "drawn": a.drawn.map { ["place": $0.place, "amount": n($0.amount)] as [String: Any] },
                                 "moves": a.moves.map { ["from": $0.from, "to": $0.to, "amount": n($0.amount)] as [String: Any] }] as [String: Any]
        }
        if let t = ws.takeChoices { out["takeChoices"] = t.map(\.title) }
        if let w = ws.awaitingAmount { out["awaitingAmount"] = String(describing: w) }
        if ws.intent != TradeIntent() {
            out["plan"] = ["asset": ws.intent.asset, "side": ws.intent.side == .long ? "long" : "short", "ready": ws.planIsReady,
                           "steps": steps(ws.planRoute(StudioNode.build(ws)).steps)] as [String: Any]
        }
        if ws.showReceive { out["receive"] = true }
        if let saved = ws.savedBoard { out["saved"] = saved.displayTitle }
        if let n = ws.noteCard { out["note"] = ["text": n.texts.last ?? "", "texts": n.texts, "folder": n.folder ?? NSNull(), "own": n.canvas == ws.canvasID] as [String: Any] }
        return out
    }

    private func json(_ object: [String: Any]) -> Data {
        (try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])) ?? Data("{}".utf8)
    }

    /// A Kamino Vault deposit as the browser saw one (2026-10-01): 5 USDC in, two of Kamino's programs not on the list.
    static var sampleReview: TxReview {
        var r = TxReview(wire: Data(), accounts: [], tokenOwners: [:], unknownPrograms: ["FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr", "KvauGMspG5k6rtzrqqn7WNn3oZdyKqLKwK2XWQ8FLjd"],
                         programNames: [], changes: [],
                         checks: ["Only your wallet signs and pays the fee.", "Creates a token account owned by your wallet."], problems: [],
                         jev: JevReview(label: "ok", confidence: 0.9, concern: false, text: "An ordinary action for this kind of site."))
        r.deltas = [("SOL", SpotAsset.all[0].mint, Decimal(string: "-0.006865555")!), ("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", -5)]
        r.programs = [("Associated Token", true), ("Compute Budget", true), ("Kamino Farms", false), ("Kamino Vault", false)]
        return r
    }

    /// A sheet drawn in an off-screen window, controls included (an ImageRenderer leaves AppKit's out).
    static func renderApproval(_ request: SiteRequest, ws: Workspace, dark: Bool) -> Data? {
        let view = NSHostingView(rootView: ApprovalSheet(request: request) { _ in }.environment(ws).background(Color(nsColor: .windowBackgroundColor)))
        view.frame.size = view.fittingSize
        let window = NSWindow(contentRect: view.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = view
        view.layoutSubtreeIfNeeded()
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep.representation(using: .png, properties: [:])
    }

    /// The main window as it is drawn, without the rest of the screen.
    private func screenshot() -> Data? {
        guard let view = NSApp.windows.first(where: { $0.isVisible && $0.title == AppMain.mainTitle })?.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep.representation(using: .png, properties: [:])
    }
}
#else
/// The Top up page still reads its requests with DebugAPI.Request.
enum DebugAPI {}
#endif

extension DebugAPI {
    /// One HTTP/1.1 request, once its headers and body have all arrived; nil while they haven't.
    struct Request: Sendable {
        let method: String
        let path: String
        let headers: [String: String]
        let body: Data

        init?(_ data: Data) {
            guard let end = data.range(of: Data("\r\n\r\n".utf8)) else { return nil }
            let lines = String(decoding: data[..<end.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
            let first = lines.first?.split(separator: " ") ?? []
            var headers: [String: String] = [:]
            for line in lines.dropFirst() {
                guard let colon = line.firstIndex(of: ":") else { continue }
                headers[line[..<colon].lowercased()] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            }
            let length = Int(headers["content-length"] ?? "0") ?? 0
            let rest = data[end.upperBound...]
            // A negative length would trap in prefix(_:) below, before any token is checked (the Top up page shares this).
            guard length >= 0, rest.count >= length else { return nil }
            method = first.first.map(String.init) ?? ""
            path = first.dropFirst().first.map(String.init) ?? ""
            self.headers = headers
            body = Data(rest.prefix(length))
        }
    }
}

import Foundation
import HelperAPI

/// Crash reports macOS writes for ihatecrypto and its signer (~/Library/Logs/DiagnosticReports, *.ips), copied into
/// Application Support/ihatecrypto/Crashes before macOS retires them, each with a one-line summary: the exception and the
/// first frame in our own code. Nothing leaves the Mac; the debug API's GET /crashes lists them for whoever fixes them.
enum Crashes {
    nonisolated struct Report: Codable {
        var file: String
        var date: Date
        var summary: String
    }

    static var folder: URL {
        let url = LocalStore.url("Crashes")
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    /// Every report kept, newest first.
    static var all: [Report] { (LocalStore.load("Crashes/index.json") as [Report]?) ?? [] }

    /// Copies reports not kept yet and returns the ones since the last launch. The first launch with this keeps the old
    /// ones quietly: they were before anyone looked.
    static func collect() -> [Report] {
        let logs = Helper.home.appending(path: "Library/Logs/DiagnosticReports")
        let files = [logs, logs.appending(path: "Retired")].flatMap { dir in
            ((try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? [])
                .filter { url in url.pathExtension == "ips" && [ProcessInfo.processInfo.processName + "-", "Signer-"].contains { url.lastPathComponent.hasPrefix($0) } }
        }
        var kept = all
        let firstLook = UserDefaults.standard.object(forKey: "crashesSeen") == nil
        var fresh: [Report] = []
        for url in files where !kept.contains(where: { $0.file == url.lastPathComponent }) {
            guard let report = read(url) else { continue }
            try? FileManager.default.copyItem(at: url, to: folder.appending(path: url.lastPathComponent))
            kept.append(report)
            if !firstLook { fresh.append(report) }
        }
        UserDefaults.standard.set(true, forKey: "crashesSeen")
        if kept.count != all.count { LocalStore.save(kept.sorted { $0.date > $1.date }, "Crashes/index.json") }
        return fresh.sorted { $0.date > $1.date }
    }

    /// «EXC_BREAKPOINT · VoiceInput.allowed() (Voice.swift)»: the first line is JSON with the time, the rest the report.
    static func read(_ url: URL) -> Report? {
        guard let text = try? String(contentsOf: url, encoding: .utf8), let cut = text.firstIndex(of: "\n"),
              let header = try? JSONSerialization.jsonObject(with: Data(text[..<cut].utf8)) as? [String: Any],
              let body = try? JSONSerialization.jsonObject(with: Data(text[cut...].utf8)) as? [String: Any] else { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH:mm:ss.SS Z"
        let date = (header["timestamp"] as? String).flatMap(f.date) ?? .distantPast
        let exception = (body["exception"] as? [String: Any])?["type"] as? String
            ?? (body["termination"] as? [String: Any])?["indicator"] as? String ?? "crash"
        let images = (body["usedImages"] as? [[String: Any]])?.map { $0["name"] as? String } ?? []
        let threads = body["threads"] as? [[String: Any]] ?? []
        let frames = (body["faultingThread"] as? Int).flatMap { $0 < threads.count ? threads[$0]["frames"] as? [[String: Any]] : nil } ?? []
        // The first frame in the app or the signer, else the first with a name.
        let ours = frames.first { f in (f["imageIndex"] as? Int).flatMap { $0 < images.count ? images[$0] : nil }.map { [ProcessInfo.processInfo.processName, "Signer"].contains($0) } ?? false }
        let frame = (ours ?? frames.first { $0["symbol"] != nil }).map { f in
            (f["symbol"] as? String ?? "?") + ((f["sourceFile"] as? String).map { " (\($0))" } ?? "")
        }
        let process = header["app_name"] as? String ?? url.lastPathComponent
        return Report(file: url.lastPathComponent, date: date, summary: "\(process): \(exception)" + (frame.map { " · \($0)" } ?? ""))
    }
}

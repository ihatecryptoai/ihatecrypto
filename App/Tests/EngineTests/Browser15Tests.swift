import Foundation
import Testing

@testable import DesktopEngine

/// The browser's wave 14 (audit13 G13-1): a phishing check never reads an empty list while the day's copy downloads.
/// With a saved copy over a day old, two checks at once answer from it at once; with none, both wait for the one
/// download. No network: the download is a stand-in that counts its calls. Public test hosts only.
@Suite(.serialized) struct Browser15Tests {
    actor Calls { var n = 0; var finished = false; func add() { n += 1 }; func finish() { finished = true } }
    static let list = Data(#"{"blacklist":["phish-test.example"],"whitelist":["ok.phish-test.example"]}"#.utf8)

    static func file(_ name: String) -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("browser15-\(name)-\(UUID().uuidString).json")
        try? FileManager.default.removeItem(at: url)
        return url
    }

    @Test func aDayOldSavedListAnswersWhileTheNewOneDownloads() async throws {
        let url = Self.file("stale")
        try Self.list.write(to: url)
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-3 * 86_400)], ofItemAtPath: url.path)
        let calls = Calls()
        let phishing = PhishingList(file: url, fetch: {
            await calls.add()
            // By order, not the clock: a full parallel run can slow any wait, never this order.
            try? await Task.sleep(for: .seconds(30))
            await calls.finish()
            return nil
        })
        async let first = phishing.contains("phish-test.example")
        async let second = phishing.contains("www.phish-test.example")
        let (a, b) = await (first, second)
        #expect(a && b)
        #expect(await !calls.finished, "a check waited for the download")
        #expect(await !phishing.contains("ok.phish-test.example"))
        #expect(await calls.n == 1)
        try? FileManager.default.removeItem(at: url)
    }

    @Test func withNothingSavedBothChecksWaitForTheOneDownload() async throws {
        let url = Self.file("none")
        let calls = Calls()
        let phishing = PhishingList(file: url, fetch: {
            await calls.add()
            try? await Task.sleep(for: .milliseconds(300))
            return Browser15Tests.list
        })
        async let first = phishing.contains("phish-test.example")
        async let second = phishing.contains("phish-test.example")
        let (a, b) = await (first, second)
        #expect(a && b)
        #expect(await calls.n == 1)
        // The download is saved for the next run.
        #expect((try? Data(contentsOf: url)) == Self.list)
        try? FileManager.default.removeItem(at: url)
    }
}

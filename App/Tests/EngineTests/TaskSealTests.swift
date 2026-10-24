@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Tasks changed outside ihatecrypto load paused (TaskSeal, the Mac's 2026-10-24 review). In-process: the engine's store
/// is this test process's own folder, set before anything else reads it.
@Test func aTaskListChangedOutsideLoadsPaused() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("taskseal-\(UUID().uuidString)")
    setEnv("IHATECRYPTO_ENGINE_DIR", dir.path)
    defer { try? FileManager.default.removeItem(at: dir) }
    let task = ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date().addingTimeInterval(3600), every: nil)
    TaskSeal.save([task])
    // As saved: intact, running.
    var loaded = TaskSeal.load()
    #expect(!loaded.tampered && loaded.tasks.first?.paused != true)
    // Another program rewrites the list: every task comes back paused, and that's said.
    var changed = task
    changed.steps = ["send 50 USDC to 0x000000000000000000000000000000000000dEaD"]
    try JSONEncoder().encode([changed]).write(to: Store.url("tasks.json"))
    loaded = TaskSeal.load()
    #expect(loaded.tampered && loaded.tasks.allSatisfy { $0.paused == true })
    // Saved again by the app (the user switched one on): intact again.
    TaskSeal.save(loaded.tasks)
    #expect(!TaskSeal.load().tampered)
}

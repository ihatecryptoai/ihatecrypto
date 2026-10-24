@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// The tests share one seal key (made by the first save): one at a time.
@Suite(.serialized) struct TaskSealTests {
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
        // Another program deletes the key and writes its own list: with no key to check it by, it loads paused too.
        try? FileManager.default.removeItem(at: Store.url("task-seal.key"))
        try JSONEncoder().encode([changed]).write(to: Store.url("tasks.json"))
        loaded = TaskSeal.load()
        #expect(loaded.tampered && loaded.tasks.allSatisfy { $0.paused == true })
    }

    /// Each wallet keeps its own tasks (the Mac's Wallets.swift): a list under its own name with its own tag, so another
    /// wallet's list changed outside ihatecrypto loads paused and this one doesn't.
    @Test func eachWalletsTasksHaveTheirOwnListAndSeal() throws {
        // The store's folder is the first one this process set (Store.folder), which the test before may have removed.
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("taskseal-\(UUID().uuidString)").path)
        try FileManager.default.createDirectory(at: Store.folder, withIntermediateDirectories: true)
        let mine = Engine.scoped("tasks.json", "7"), other = Engine.scoped("tasks.json", "8")
        #expect(mine == "tasks-7.json" && TaskSeal.sealName(mine) == "tasks-7.seal" && Engine.scoped("tasks.json", SignerWallet.firstID) == "tasks.json")
        let task = ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date().addingTimeInterval(3600), every: nil)
        TaskSeal.save([task], mine)
        TaskSeal.save([task, task], other)
        #expect(TaskSeal.load(mine).tasks.count == 1 && TaskSeal.load(other).tasks.count == 2)
        var changed = task
        changed.steps = ["send 50 USDC to 0x000000000000000000000000000000000000dEaD"]
        try JSONEncoder().encode([changed]).write(to: Store.url(other))
        #expect(TaskSeal.load(other).tampered && !TaskSeal.load(mine).tampered)
    }
}

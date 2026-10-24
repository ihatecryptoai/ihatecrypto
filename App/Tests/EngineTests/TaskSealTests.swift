@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// One at a time; and each test seals files of its own names, never the app's tasks.json and key, which other in-process
/// suites write while these run (the store's folder is one for the whole test process: Store.folder).
@Suite(.serialized) struct TaskSealTests {
    /// Fresh names for a list and its key in the store's folder.
    static func names() throws -> (list: String, key: String) {
        try FileManager.default.createDirectory(at: Store.folder, withIntermediateDirectories: true)
        let id = UUID().uuidString.prefix(8)
        return ("tasks-t\(id).json", "seal-t\(id).key")
    }

    /// Tasks changed outside ihatecrypto load paused (TaskSeal, the Mac's 2026-10-24 review).
    @Test func aTaskListChangedOutsideLoadsPaused() throws {
        let (list, key) = try Self.names()
        let task = ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date().addingTimeInterval(3600), every: nil)
        TaskSeal.save([task], list, keyFile: key)
        // As saved: intact, running.
        var loaded = TaskSeal.load(list, keyFile: key)
        #expect(!loaded.tampered && loaded.tasks.first?.paused != true)
        // Another program rewrites the list: every task comes back paused, and that's said.
        var changed = task
        changed.steps = ["send 50 USDC to 0x000000000000000000000000000000000000dEaD"]
        try JSONEncoder().encode([changed]).write(to: Store.url(list))
        loaded = TaskSeal.load(list, keyFile: key)
        #expect(loaded.tampered && loaded.tasks.allSatisfy { $0.paused == true })
        // Saved again by the app (the user switched one on): intact again.
        TaskSeal.save(loaded.tasks, list, keyFile: key)
        #expect(!TaskSeal.load(list, keyFile: key).tampered)
        // Another program deletes the key and writes its own list: with no key to check it by, it loads paused too.
        try? FileManager.default.removeItem(at: Store.url(key))
        try JSONEncoder().encode([changed]).write(to: Store.url(list))
        loaded = TaskSeal.load(list, keyFile: key)
        #expect(loaded.tampered && loaded.tasks.allSatisfy { $0.paused == true })
    }

    /// Each wallet keeps its own tasks (the Mac's Wallets.swift): a list under its own name with its own tag, so another
    /// wallet's list changed outside ihatecrypto loads paused and this one doesn't.
    @Test func eachWalletsTasksHaveTheirOwnListAndSeal() throws {
        let (_, key) = try Self.names()
        #expect(Engine.scoped("tasks.json", "7") == "tasks-7.json" && TaskSeal.sealName("tasks-7.json") == "tasks-7.seal"
                && Engine.scoped("tasks.json", SignerWallet.firstID) == "tasks.json")
        let mine = try Self.names().list, other = try Self.names().list
        let task = ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date().addingTimeInterval(3600), every: nil)
        TaskSeal.save([task], mine, keyFile: key)
        TaskSeal.save([task, task], other, keyFile: key)
        #expect(TaskSeal.load(mine, keyFile: key).tasks.count == 1 && TaskSeal.load(other, keyFile: key).tasks.count == 2)
        var changed = task
        changed.steps = ["send 50 USDC to 0x000000000000000000000000000000000000dEaD"]
        try JSONEncoder().encode([changed]).write(to: Store.url(other))
        #expect(TaskSeal.load(other, keyFile: key).tampered && !TaskSeal.load(mine, keyFile: key).tampered)
    }
}

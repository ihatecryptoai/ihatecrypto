@testable import IhatecryptoApp
import CryptoKit
import Foundation
import IhatecryptoCore
import Testing

/// Recurring buys whose seal didn't hold load paused, every one, and say so once; sealed ones load as saved. Only what
/// needs no Keychain: the key here is made in memory.
@Test func recurringBuysChangedOutsideLoadPaused() throws {
    let mine = Schedule(symbol: "SOL", usdc: 10, days: 7, next: Date(timeIntervalSince1970: 1_800_000_000))
    let saved = try JSONEncoder().encode([mine])
    // What another program writes into the preferences: the user's buy made bigger, and one of its own, due now.
    let changed = try JSONEncoder().encode([Schedule(symbol: "SOL", usdc: 5000, days: 7, next: mine.next), Schedule(symbol: "BTC", usdc: 5000, days: 1, next: .distantPast)])

    let key = SymmetricKey(size: .bits256)
    #expect(TaskSeal.tag(saved, key) == TaskSeal.tag(saved, key) && TaskSeal.tag(changed, key) != TaskSeal.tag(saved, key))
    // The tag covers the name the buys are saved under: one wallet's sealed list doesn't hold under another wallet's
    // name. The tasks' tags, in use before that, cover the bytes alone.
    func tag(_ name: String) -> Data { TaskSeal.tag(TaskSeal.sealed(saved, name), key) }
    #expect(tag("schedules") != tag("schedules-2") && tag("exitRules") != tag("exitRules-2") && tag("schedules") != tag("exitRules"))
    #expect(tag("tasks") == tag("tasks-2") && tag("tasks") == TaskSeal.tag(saved, key))

    let intact = TaskSeal.schedules(saved, intact: true)
    #expect(intact.schedules == [mine] && !intact.tampered)
    let loaded = TaskSeal.schedules(changed, intact: false)
    #expect(loaded.schedules.map(\.symbol) == ["SOL", "BTC"] && loaded.schedules.allSatisfy { $0.paused == true } && loaded.tampered)
    // Nothing saved, or an empty list: nothing to pause and nothing to say.
    #expect(TaskSeal.schedules(nil, intact: true).schedules.isEmpty && !TaskSeal.schedules(nil, intact: true).tampered)
    #expect(!TaskSeal.schedules(try JSONEncoder().encode([Schedule]()), intact: false).tampered)
}

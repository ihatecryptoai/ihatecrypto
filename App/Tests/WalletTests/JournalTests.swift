import ExecutionCore
import Foundation
import Testing

@Test func journalKeepsOrderAndFindsUnresolvedSends() throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("journal-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    try journal.append(.init(chain: "solana", id: "sigA", kind: .signed, note: "swap"))
    try journal.append(.init(chain: "solana", id: "sigB", kind: .signed, note: "bridge deposit"))
    try journal.append(.init(chain: "solana", id: "sigA", kind: .confirmed))
    try journal.append(.init(chain: "1click", id: "depositX", kind: .signed))

    #expect(try journal.entries().count == 4)
    #expect(try journal.unresolved().map(\.id) == ["sigB", "depositX"])
    let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
    #expect((attributes[.posixPermissions] as? Int) == 0o600)
}

@Test func aSendWrittenNowIsFoundFromTheSecondItStarted() throws {
    // The app asks which sends since a move started have no outcome (Execution.swift unsettled); the journal keeps whole
    // seconds, so the start is floored the same way or a send in that second is missed.
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("journal-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    let started = Date()
    try journal.append(.init(chain: "base", id: "0xabc", kind: .signed, note: "Relay deposit"))
    let from = Date(timeIntervalSince1970: started.timeIntervalSince1970.rounded(.down))
    #expect(try journal.unresolved().filter { $0.at >= from }.map(\.id) == ["0xabc"])
}

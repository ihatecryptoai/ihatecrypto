import Foundation
@testable import IhatecryptoApp
import Testing

/// N5: the journey follows 1Click's states as Executor.oneClickSettled words them, never goes back, and ends honestly.
@Test func theJourneyFollowsOneClicksStates() {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    var j = IntentsJourney(title: "2 USDC → Automation", from: "base", to: "solana", privately: true, estimate: nil, started: t0)
    j.read("NEAR Intents (private): 2.02 USDC from Base → 2 USDC to «Automation»", now: t0.addingTimeInterval(1))
    #expect(j.stage == .paying && j.clock(t0) == "signing")
    j.read("1Click · deposit sent, waiting for it to land · 0 s of ≈ 37", now: t0.addingTimeInterval(4))
    #expect(j.stage == .paid && j.estimate == 37 && j.paidAt == t0.addingTimeInterval(4))
    #expect(j.clock(t0.addingTimeInterval(14)) == "≈ 27 s left")
    j.read("1Click · deposit received, swapping · 12 s of ≈ 37", now: t0.addingTimeInterval(16))
    #expect(j.stage == .filling)
    // A late «waiting» line doesn't take it back.
    j.read("1Click · deposit sent, waiting for it to land · 13 s of ≈ 37", now: t0.addingTimeInterval(17))
    #expect(j.stage == .filling && j.paidAt == t0.addingTimeInterval(4))
    #expect(j.clock(t0.addingTimeInterval(60)) == "any moment now")
    j.read("1Click · done · 41 s of ≈ 37", now: t0.addingTimeInterval(45))
    #expect(j.stage == .landed && j.clock(t0.addingTimeInterval(99)) == "landed in 45 s" && j.progress(t0) == 1)
}

@Test func aRefundIsSaidAndAFailureBeforePayingIsNoJourney() {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    var j = IntentsJourney(title: "1 USDC → Solana", from: "base", to: "solana", privately: false, estimate: 30, started: t0)
    #expect(j.ended(ok: false, now: t0) == nil)
    // Sent with no outcome (the journal holds the deposit): paid and still on its way, never dropped.
    #expect(j.ended(ok: false, unknown: true, now: t0)?.stuck == true)
    j.read("1Click · deposit sent, waiting for it to land · 0 s of ≈ 30", now: t0.addingTimeInterval(3))
    // Ended without 1Click's last word: still on its way, not «failed».
    let lost = j.ended(ok: false, now: t0.addingTimeInterval(600))
    #expect(lost?.stuck == true && lost?.clock(t0) == "still on its way")
    // A short deposit isn't the end: 1Click refunds it and says so next.
    j.read("1Click · the deposit came short: 1Click refunds it · 40 s of ≈ 30", now: t0.addingTimeInterval(45))
    #expect(j.stage == .paid && j.endedAt == nil)
    var failed = j
    failed.read("1Click · failed · 60 s of ≈ 30", now: t0.addingTimeInterval(65))
    #expect(failed.stage == .refunded && failed.clock(t0) == "failed")
    j.read("1Click · refunded · 70 s of ≈ 30", now: t0.addingTimeInterval(75))
    #expect(j.stage == .refunded && j.clock(t0) == "refunded" && j.endedAt != nil && j.ended(ok: false)?.stuck == false)
    #expect(IntentsJourney.duration(150) == "2:30")
}

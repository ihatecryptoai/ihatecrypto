import Foundation
@testable import GuardJev
import Testing

private let labels: KeyValuePairs = ["matches": "It does.", "mismatch": "It doesn't.", "unclear": "Can't tell."]

/// Order is part of the question: the state as listed, the options once as listed and once reversed.
@Test func theRequestKeepsItsOrder() throws {
    let body = String(decoding: try JevClassifier.body(state: ["forms": "lend = Jupiter Lend", "recent_chat": [String](), "message": "депозит «15$»", "command": "lend 15 USDC"],
                                                       instructions: "Judge.", criteria: labels, asks: ["asset": "Same asset?"]), as: UTF8.self)
    func at(_ s: String) -> String.Index { body.range(of: s)?.lowerBound ?? body.endIndex }
    #expect(at("\"forms\"") < at("\"message\"") && at("\"message\"") < at("\"command\""))
    #expect(!body.contains("recent_chat"))   // an empty list isn't sent
    let a = body[at("\"a\":")..<at("\"b\":")], b = body[at("\"b\":")..<at("\"asset\":")]
    #expect(a.range(of: "\"matches\"")!.lowerBound < a.range(of: "\"unclear\"")!.lowerBound)
    #expect(b.range(of: "\"unclear\"")!.lowerBound < b.range(of: "\"matches\"")!.lowerBound)
    // And it is still JSON, the user's quotes and Cyrillic intact.
    let parsed = try JSONSerialization.jsonObject(with: Data(body.utf8)) as? [String: Any]
    #expect(((parsed?["state"] as? [String: Any])?["message"] as? String) == "депозит «15$»")
    #expect(((parsed?["questions"] as? [String: Any])?["asset"] as? [String: String])?["type"] == "noul")
}

@Test func theTwoOrdersAreAveraged() throws {
    let reply = #"{"answers":{"a":{"probabilities":{"matches":0.85,"mismatch":0.14,"unclear":0.01}},"b":{"probabilities":{"unclear":0,"mismatch":0.06,"matches":0.95}},"asset":{"noul":0.97}}}"#
    let (p, yes) = try JevClassifier.answer(Data(reply.utf8), criteria: labels, asks: ["asset": "Same asset?", "place": "Same place?"])
    #expect(abs(p["matches"]! - 0.9) < 1e-9 && abs(p["mismatch"]! - 0.1) < 1e-9)
    #expect(yes == ["asset": 0.97])   // an ask that didn't come back is only a missing reason
    // One order without an option's probability is no answer.
    let half = #"{"answers":{"a":{"probabilities":{"matches":0.9,"mismatch":0.1,"unclear":0}},"b":{"probabilities":{"matches":0.9}}}}"#
    #expect(throws: JevError.self) { try JevClassifier.answer(Data(half.utf8), criteria: labels, asks: [:]) }
}

/// The screen's answer: «data» with its probability, or the heavier of the other two with what both hold.
@Test func theScreenAddsUpWhatIsNotData() {
    let split = JevClassifier.verdict(["data": 0.2, "instructions": 0.35, "payment_redirect": 0.45])
    #expect(split.label == "payment_redirect" && abs(split.confidence - 0.8) < 1e-9)
    let plain = JevClassifier.verdict(["data": 0.9, "instructions": 0.06, "payment_redirect": 0.04])
    #expect(plain.label == "data" && abs(plain.confidence - 0.9) < 1e-9)
    #expect(JevClassifier.verdict(["data": 0.55, "instructions": 0.45, "payment_redirect": 0]).label == "data")
}

/// The verdict is the probability left for the good answer, not the top label and its distance from an even split.
@Test func aSplitDoubtIsStillADoubt() {
    let texts = ["matches": "matches", "mismatch": "differs", "risky": "risky"]
    // 45 / 40 against, 15 for: the old rule read «mismatch, confidence 0.18» and wrote that it passed.
    let split = JevReviewer.review(["matches": 0.15, "mismatch": 0.45, "risky": 0.40], good: "matches", texts: texts)
    #expect(split.concern && split.label == "mismatch" && abs(split.confidence - 0.85) < 1e-9)
    let fine = JevReviewer.review(["matches": 0.77, "mismatch": 0.13, "risky": 0.10], good: "matches", texts: texts)
    #expect(!fine.concern && fine.label == "matches" && fine.text == "matches.")
    #expect(JevReviewer.review(["matches": 0.5, "mismatch": 0.25, "risky": 0.25], good: "matches", texts: texts).concern)
}

/// A mismatch names what differs — the ask least likely «yes», under one half — as data the app words in the chat's language.
@Test func readingReviewNamesWhatDiffers() {
    let wrong = JevReviewer.readingReview(["matches": 0.1, "mismatch": 0.85, "unclear": 0.05], yes: ["action": 0.2, "asset": 0.9, "place": 0.7, "amount": 0.95])
    #expect(wrong.concern && wrong.about == "action" && wrong.text == "the command does something other than the message asks: the action isn't the one asked for.")
    let right = JevReviewer.readingReview(["matches": 0.9, "mismatch": 0.08, "unclear": 0.02], yes: ["action": 0.2])
    #expect(!right.concern && right.about == nil)
    // Every ask said «yes»: a mismatch with nothing named.
    let vague = JevReviewer.readingReview(["matches": 0.2, "mismatch": 0.7, "unclear": 0.1], yes: ["action": 0.9, "asset": 0.8])
    #expect(vague.concern && vague.about == nil)
}

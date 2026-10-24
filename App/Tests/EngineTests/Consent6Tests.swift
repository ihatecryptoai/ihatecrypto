@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// Consent as the Mac asks it (audit4 A1, A2, A4, A8): a site's request in its own block beside the person's card,
/// answered by its id and only with a tick for each risk; Confirm and Cancel only for the card they were drawn for; a
/// wallet switch dropping what was built for the other wallet; a picked card run against the message it answered.
/// In-process, public test data only, nothing goes to the network or the signer.
@MainActor @Suite(.serialized) struct Consent6Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("consent-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    // Hardhat's public accounts #1 and #3, and a public contract (AERO on Base).
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let phone = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    static let contract = "0x940181a94A35A4569E4529A3CDfB74e38FD98631"
    static let en: (String, String) -> String = { a, _ in a }

    private var engine: Engine { Self.engine }

    private func fresh() {
        engine.settings.language = "en"
        engine.cancelPending()
        engine.declineSite("test")
        engine.parked = [:]
        engine.awaitingAmount = nil
    }

    private func card(_ name: String, _ address: String) {
        engine.show(.address(SignerAddress(name: name, network: "base", address: address)), title: "Save “\(name)” to Addresses")
    }

    private func waitForSite() async throws -> SiteAskView {
        for _ in 0..<200 {
            if let v = engine.siteAsk?.view { return v }
            try await Task.sleep(for: .milliseconds(5))
        }
        throw WalletErrorText("the site's request never came")
    }

    private func status(_ active: String) throws -> SignerReply {
        var r = SignerReply(ok: true)
        r.hasWallet = true
        r.activeWallet = active
        return r
    }

    @Test func aSitesAskWaitsBesideTheCardNeverInItsPlace() async throws {
        fresh()
        card("Mom", Self.mom)
        let id = try #require(engine.card?.id)
        let asked = Task { await engine.ask("jup.ag", origin: "https://jup.ag", site: .known, connect: true, title: "jup.ag asks to connect", lines: []) }
        let site = try await waitForSite()
        #expect(engine.card?.id == id)
        if case .address? = engine.pending {} else { Issue.record("the person's card was replaced") }
        #expect(site.approve == "Connect" && site.ticks.isEmpty && site.origin == "https://jup.ag")
        // The card's Confirm aimed at the site's block does nothing to either.
        await engine.confirm(card: site.id)
        #expect(engine.card?.id == id && engine.siteAsk?.view.id == site.id && engine.runningBoard == nil)
        // One request at a time: a second one while this waits is declined.
        #expect(await engine.ask("jup.ag", origin: "https://jup.ag", site: .known, title: "again", lines: []) == false)
        // A new card of the person's leaves the site's request waiting.
        card("Phone", Self.phone)
        #expect(engine.siteAsk?.view.id == site.id)
        try engine.answerSite(site.id, yes: true, ticks: [])
        #expect(await asked.value == true)
        #expect(engine.siteAsk == nil && engine.card != nil)
        #expect(engine.boards.first { $0.id == site.id }?.state == "done")
        engine.cancelPending()
    }

    @Test func confirmAndCancelAnswerOnlyTheirOwnCard() async throws {
        fresh()
        card("Mom", Self.mom)
        let first = try #require(engine.card?.id)
        card("Phone", Self.phone)
        let second = try #require(engine.card?.id)
        await engine.confirm(card: first)
        #expect(engine.card?.id == second && engine.runningBoard == nil)
        engine.cancelPending(card: first)
        #expect(engine.card?.id == second)
        engine.cancelPending(card: second)
        #expect(engine.card == nil)
    }

    @Test func aSiteRequestNeedsATickForEachRisk() async throws {
        fresh()
        let payee = try #require(Engine.payeeTick(to: Self.contract, recipients: [Self.mom.lowercased()], unread: false, say: Self.en))
        #expect(payee.id == "payee" && payee.toggle == "Send to this address")
        #expect(Engine.payeeTick(to: Self.contract, recipients: [Self.contract.lowercased()], unread: true, say: Self.en)?.id == "contract")
        #expect(Engine.payeeTick(to: Self.contract, recipients: [], unread: false, say: Self.en) == nil)
        #expect(Engine.programsTick([], say: Self.en) == nil)
        #expect(Engine.programsTick(["JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"], say: Self.en)?.toggle == "Allow it for this one transaction")

        let asked = Task {
            await engine.ask("app-uniswap.org", origin: "https://app-uniswap.org", site: .lookalike("app.uniswap.org"), title: "app-uniswap.org asks to send a transaction",
                             lines: [], ticks: [payee, Engine.jevTick("It pays an address the site named.", say: Self.en)])
        }
        let site = try await waitForSite()
        #expect(site.ticks.map(\.id) == ["lookalike", "payee", "jev"])
        #expect(site.check == "lookalike" && site.real == "app.uniswap.org" && site.approve == "Sign")
        for partial in [[], ["lookalike"], ["lookalike", "payee"], ["payee", "jev"]] {
            #expect(throws: (any Error).self) { try engine.answerSite(site.id, yes: true, ticks: partial) }
            #expect(engine.siteAsk?.view.id == site.id)
        }
        // Another block's id answers nothing.
        try engine.answerSite("another", yes: true, ticks: ["lookalike", "payee", "jev"])
        #expect(engine.siteAsk?.view.id == site.id)
        try engine.answerSite(site.id, yes: true, ticks: ["lookalike", "payee", "jev"])
        #expect(await asked.value == true)

        // Declining needs no tick.
        let declined = Task { await engine.ask("app-uniswap.org", origin: "https://app-uniswap.org", site: .lookalike("app.uniswap.org"), title: "again", lines: []) }
        let next = try await waitForSite()
        try engine.answerSite(next.id, yes: false, ticks: [])
        #expect(await declined.value == false)
        #expect(engine.boards.first { $0.id == next.id }?.state == "cancelled")
    }

    @Test func anotherWalletDropsWhatWasBuiltForTheLast() async throws {
        fresh()
        engine.statusHeard(try status("1"))
        card("Mom", Self.mom)
        let id = try #require(engine.card?.id)
        engine.awaitingAmount = WalletCommand.parse("buy SOL")
        engine.parked["another canvas"] = (try #require(engine.pending), try #require(engine.card), TradeIntent(), Date())
        let asked = Task { await engine.ask("jup.ag", origin: "https://jup.ag", site: .known, connect: true, title: "jup.ag asks to connect", lines: []) }
        _ = try await waitForSite()
        // The same wallet read again: nothing goes.
        engine.statusHeard(try status("1"))
        #expect(engine.card?.id == id && engine.siteAsk != nil)
        engine.statusHeard(try status("2"))
        #expect(engine.card == nil && engine.pending == nil && engine.parked.isEmpty && engine.awaitingAmount == nil && engine.siteAsk == nil)
        #expect(await asked.value == false)
        #expect(engine.boards.first { $0.id == id }?.state == "cancelled")
        #expect(engine.boards.first { $0.id == id }?.lines.last?.contains("other wallet") == true)
    }

    @Test func aPickedCardRunsAsItIsAgainstTheMessageItAnswered() async throws {
        fresh()
        let save = "save \(Self.mom) as Mom on base"
        engine.offer([ChatRouter.Option(title: "Save Mom", comment: "", command: save)], userText: "keep \(Self.mom) as Mom", found: [])
        #expect(engine.options.count == 1)
        await engine.pick(0)
        #expect(engine.card?.title.contains("Mom") == true, "\(engine.messages.last?.text ?? "")")
        #expect(engine.messages.last(where: \.fromUser)?.text == "Save Mom")
        engine.cancelPending()
        // Cards that weren't offered for a message of the user's bring none of their words: an address in one isn't taken.
        engine.options = [ChatRouter.Option(title: "Save Mom", comment: "", command: save)]
        await engine.pick(0)
        #expect(engine.card == nil)
        #expect(engine.messages.last?.text.contains("can't do") == true, "\(engine.messages.last?.text ?? "")")
    }
}

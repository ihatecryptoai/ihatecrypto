import CWinSec
import Foundation
import Testing

/// Settings › Modules and Settings › Trading agent, as on the Mac: a switched-off protocol, kind or network is refused
/// before any card, a module a later version brings starts off and is asked about once, and the trading rules stay within
/// the Mac's bounds.
@Suite(.serialized) struct ModulesTests {
    static func modules(_ state: [String: Any]?) -> [String: Any] { state?["modules"] as? [String: Any] ?? [:] }
    static func trading(_ state: [String: Any]?) -> [String: Any] { state?["trading"] as? [String: Any] ?? [:] }
    static func protocolOn(_ state: [String: Any]?, _ id: String) -> Bool? {
        for k in modules(state)["kinds"] as? [[String: Any]] ?? [] {
            for p in k["protocols"] as? [[String: Any]] ?? [] where p["id"] as? String == id { return p["on"] as? Bool }
        }
        return nil
    }
    static func networkOn(_ state: [String: Any]?, _ id: String) -> Bool? {
        (modules(state)["networks"] as? [[String: Any]])?.first { $0["id"] as? String == id }?["on"] as? Bool
    }
    static func allIDs(_ state: [String: Any]?) -> [String] {
        let m = modules(state)
        let kinds = m["kinds"] as? [[String: Any]] ?? []
        return (m["networks"] as? [[String: Any]] ?? []).compactMap { $0["id"] as? String } + kinds.compactMap { $0["id"] as? String }
            + kinds.flatMap { ($0["protocols"] as? [[String: Any]] ?? []).compactMap { $0["id"] as? String } }
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func aSwitchedOffModuleIsRefusedBeforeACard() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")

        #expect(try engine.call("module", ["name": "protocol.nothing", "test": false]).answer["ok"] as? Bool == false)
        let off = try engine.call("module", ["name": "protocol.jitoSOL", "test": false])
        #expect(Self.protocolOn(off.state, "protocol.jitoSOL") == false)
        let refused = try engine.call("send", ["text": "stake 1 SOL"])
        #expect(refused.state?["card"] == nil)
        #expect(EngineTests.chat(refused.state).last?.contains("JitoSOL staking is off in Settings › Modules") == true, "\(EngineTests.chat(refused.state).last ?? "")")

        // The kind's switch covers its protocols, and a network's covers what runs there.
        try engine.call("module", ["name": "protocol.jitoSOL", "test": true])
        try engine.call("module", ["name": "kind.earn", "test": false])
        let kind = try engine.call("send", ["text": "stake 1 SOL"])
        #expect(EngineTests.chat(kind.state).last?.contains("Earn · DeFi is off in Settings › Modules") == true, "\(EngineTests.chat(kind.state).last ?? "")")
        try engine.call("module", ["name": "kind.earn", "test": true])
        let net = try engine.call("module", ["name": "network.solana", "test": false])
        #expect(Self.networkOn(net.state, "network.solana") == false)
        let solana = try engine.call("send", ["text": "stake 1 SOL"])
        #expect(EngineTests.chat(solana.state).last?.contains("Solana") == true && solana.state?["card"] == nil)

        // All on again: the same words make a card.
        try engine.call("module", ["name": "network.solana", "test": true])
        let allowed = try engine.call("send", ["text": "stake 1 SOL"])
        #expect(allowed.state?["card"] != nil, "\(EngineTests.chat(allowed.state).last ?? "")")
    }

    @Test func aModuleALaterVersionBringsStartsOffAndIsAskedAboutOnce() throws {
        // The first run records every module there is, so nothing changes for someone who has them all.
        let first = try EngineTests.Engine()
        let known = try first.call("hello")
        let ids = Self.allIDs(known.state)
        #expect(ids.contains("network.bnb") && ids.contains("protocol.kamino") && ids.contains("kind.perps"))
        #expect(!EngineTests.chat(known.state).contains { $0.hasPrefix("New in ihatecrypto") })

        let settings = try JSONSerialization.data(withJSONObject: ["modulesKnown": ids.filter { $0 != "network.bnb" }])
        let later = try EngineTests.Engine(seed: ["settings.json": settings])
        let hello = try later.call("hello")
        #expect(Self.networkOn(hello.state, "network.bnb") == false)
        #expect(EngineTests.chat(hello.state).last?.contains("New in ihatecrypto: BNB Chain") == true, "\(EngineTests.chat(hello.state).last ?? "")")
        let options = hello.state?["options"] as? [[String: Any]] ?? []
        #expect(options.count == 2 && options.first?["title"] as? String == "Turn on BNB Chain")
        let on = try later.call("pick", ["index": 0])
        #expect(Self.networkOn(on.state, "network.bnb") == true)
        #expect(EngineTests.chat(on.state).last == "BNB Chain is on.")
    }

    @Test func theTradingRulesStayWithinTheMacsBounds() throws {
        let engine = try EngineTests.Engine()
        let hello = try engine.call("hello")
        let trading = Self.trading(hello.state)
        var policy = try #require(trading["policy"] as? [String: Any])
        #expect((policy["maxRiskPct"] as? NSNumber)?.doubleValue == 2 && (policy["maxLeverage"] as? NSNumber)?.doubleValue == 5)
        #expect((trading["styles"] as? [[String: Any]])?.compactMap { $0["id"] as? String } == ["careful", "balanced", "bold"])
        #expect(trading["safeSearch"] as? Bool == true && trading["batches"] as? Bool == false && (trading["contractCap"] as? NSNumber)?.doubleValue == 50)

        policy["maxRiskPct"] = 50
        policy["defaultRiskPct"] = 40
        policy["maxLeverage"] = 100
        policy["minRewardRisk"] = 3
        policy["defaultRewardRisk"] = 2
        let set = try engine.call("policy", ["policy": policy])
        #expect(set.answer["ok"] as? Bool == true, "\(set.answer)")
        let kept = try #require(Self.trading(set.state)["policy"] as? [String: Any])
        #expect((kept["maxRiskPct"] as? NSNumber)?.doubleValue == 10)
        #expect((kept["defaultRiskPct"] as? NSNumber)?.doubleValue == 10)
        #expect((kept["maxLeverage"] as? NSNumber)?.doubleValue == 20)
        #expect((kept["defaultRewardRisk"] as? NSNumber)?.doubleValue == 3)
        #expect((kept["maxOpenRiskPct"] as? NSNumber)?.doubleValue == 10)

        #expect(try engine.call("contractCap", ["value": 77]).answer["ok"] as? Bool == false)
        let cap = try engine.call("contractCap", ["value": 250])
        #expect((Self.trading(cap.state)["contractCap"] as? NSNumber)?.doubleValue == 250)
        let search = try engine.call("safeSearch", ["test": false])
        #expect(Self.trading(search.state)["safeSearch"] as? Bool == false)
        let batches = try engine.call("batches", ["test": true])
        #expect(Self.trading(batches.state)["batches"] as? Bool == true)
    }

    /// «By the book» fills in what isn't named from the rules (the stop by volatility, the take), as the Mac plans it: a
    /// plan with only its risk named isn't sent back for a stop.
    @Test(.enabled(if: winsec_is_stub() == 1)) func byTheBookTakesTheStopFromTheRules() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        let r = try engine.call("send", ["text": "long ETH risk $5 by the book"])
        let said = EngineTests.chat(r.state).last ?? ""
        #expect(!said.contains("stop in %") && !said.contains("The plan needs"), "\(said)")
    }

    /// A node of one's own is checked before it's kept, and only its host ever reaches the window.
    @Test func anOwnNodeIsCheckedFirst() throws {
        let engine = try EngineTests.Engine()
        let hello = try engine.call("hello")
        let nodes = try #require((hello.state?["nodes"] as? [String: Any])?["nodes"] as? [[String: Any]])
        #expect(nodes.compactMap { $0["id"] as? String } == ["solana", "base", "arbitrum", "ethereum", "polygon", "robinhood"])
        #expect(nodes.allSatisfy { $0["host"] == nil })
        let plain = try engine.call("node", ["network": "solana", "address": "http://example.com/?api-key=secret"])
        #expect(plain.answer["ok"] as? Bool == false && (plain.answer["error"] as? String)?.contains("https") == true, "\(plain.answer)")
        #expect(try engine.call("node", ["network": "dogechain", "address": "https://example.com"]).answer["ok"] as? Bool == false)
        let removed = try engine.call("oneClickKey", ["key": ""])
        #expect(removed.answer["ok"] as? Bool == true && (removed.state?["nodes"] as? [String: Any])?["oneClickKey"] as? Bool == false)
        let text = String(decoding: try JSONSerialization.data(withJSONObject: removed.state ?? [:]), as: UTF8.self)
        #expect(!text.contains("api-key=secret"))
    }

    /// Hyperliquid isn't open on a new wallet: turning the trading key on says what opens it, before any prompt.
    @Test(.enabled(if: winsec_is_stub() == 1)) func theTradingKeyWaitsForHyperliquidToOpen() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        let saved = try engine.call("phraseSaved")
        let key = try #require(Self.trading(saved.state)["key"] as? [String: Any])
        #expect(key["possible"] as? Bool == true && key["address"] == nil && key["active"] as? Bool == false)
        let asked = try engine.call("send", ["text": "turn on the trading key"])
        #expect(EngineTests.chat(asked.state).last?.contains("Hyperliquid isn't open on this wallet yet") == true, "\(EngineTests.chat(asked.state).last ?? "")")
        #expect((asked.state?["options"] as? [[String: Any]])?.contains { $0["title"] as? String == "Top up ihatecrypto" } == true)
        #expect((Self.trading(asked.state)["key"] as? [String: Any])?["address"] == nil)
    }
}

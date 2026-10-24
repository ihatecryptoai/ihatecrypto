@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// The blocks, wave 3: the token card's numbers, the address book's check as it's typed and its extras (picture,
/// category, note), and a coin's card opened twice staying one card. Public addresses only (Hardhat's first account,
/// Circle's USDC).

@Test func theTokenCardsNumbersReadAsTheMacs() {
    #expect(Engine.compactUSD(1_234_000_000) == "$1.2B")
    #expect(Engine.compactUSD(34_500_000) == "$34.5M")
    #expect(Engine.compactUSD(12_000) == "$12K")
    #expect(Engine.compactUSD(950).hasPrefix("$950"))
    #expect(Engine.shortName("Space Exploration Technologies Corp. Class A Common Stock") == "Space Exploration Technologies")
    #expect(Engine.shortName("Bonk") == "Bonk")
}

@Test func eachNetworksOwnDollarIsKnown() {
    #expect(Engine.walletDollar("solana") == SpotAsset.usdc.mint)
    #expect(Engine.walletDollar("base")?.lowercased() == "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913")
    #expect(Engine.walletDollar("near") == nil)
    #expect(Engine.extraKey(name: "Mom", network: "base") == "base:mom")
}

@Suite(.serialized) struct BlocksTests {
    static let evm = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    static let mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

    static func check(_ engine: EngineTests.Engine, _ address: String, _ network: String) throws -> [String: String] {
        try #require(try engine.call("checkAddress", ["address": address, "network": network]).answer["result"] as? [String: String])
    }

    @Test func anAddressIsCheckedForItsNetworkAsItsTyped() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        #expect(try Self.check(engine, "  ", "solana").isEmpty)
        #expect(try Self.check(engine, Self.evm, "nowhere")["problem"] != nil)
        // An EVM address under Solana, and a Solana one under Base, say which network they belong to.
        #expect(try Self.check(engine, Self.evm, "solana")["problem"]?.contains("EVM address") == true)
        #expect(try Self.check(engine, Self.mint, "base")["problem"]?.contains("pick Solana") == true)
        let ok = try Self.check(engine, Self.evm, "base")
        #expect(ok["problem"] == nil && ok["start"] == "0xf3 9Fd6" && ok["end"] == "fFb9 2266", "\(ok)")
        #expect(ok["duplicate"] == nil && ok["own"] == nil)
    }

    @Test func aSavedAddressKeepsItsPictureCategoryAndNote() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let icon = String(data: try JSONSerialization.data(withJSONObject: ["kind": "emoji", "value": "🦄"]), encoding: .utf8)!
        try engine.call("addressIcon", ["name": "Mom", "network": "base", "text": icon])
        let s = try #require(try engine.call("addressExtra", ["name": "Mom", "network": "base", "label": "Friend", "text": "Birthday money"]).state)
        let extra = try #require((s["addressExtras"] as? [String: Any])?["base:mom"] as? [String: Any])
        #expect((extra["icon"] as? [String: Any])?["value"] as? String == "🦄")
        #expect(extra["category"] as? String == "Friend" && extra["note"] as? String == "Birthday money")
        // A picture the window shouldn't send is refused; clearing everything leaves nothing behind.
        let bad = String(data: try JSONSerialization.data(withJSONObject: ["kind": "image", "value": "https://example.com/x.png"]), encoding: .utf8)!
        #expect(try engine.call("addressIcon", ["name": "Mom", "network": "base", "text": bad]).answer["ok"] as? Bool == false)
        try engine.call("addressIcon", ["name": "Mom", "network": "base", "text": ""])
        let cleared = try #require(try engine.call("addressExtra", ["name": "Mom", "network": "base", "label": "", "text": ""]).state)
        #expect((cleared["addressExtras"] as? [String: Any])?["base:mom"] == nil)
    }

    @Test func aTokensCardOpensOnceAndTheDollarRowFindsItsContract() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        // A dollar row has no contract: the network's own dollar is opened.
        let first = try engine.call("openToken", ["address": "", "network": "solana"])
        let id = try #require((first.answer["result"] as? [String: String])?["board"])
        let card = try #require(((first.state?["boards"] as? [[String: Any]])?.first { $0["id"] as? String == id }?["data"] as? [String: Any])?["token"] as? [String: Any])
        #expect(card["address"] as? String == Self.mint && card["dollar"] as? Bool == true)
        let again = try engine.call("openToken", ["address": Self.mint, "network": "solana"])
        #expect((again.answer["result"] as? [String: String])?["board"] == id)
        let tokens = (again.state?["boards"] as? [[String: Any]] ?? []).filter { $0["kind"] as? String == "token" }
        #expect(tokens.count == 1 && tokens.first?["id"] as? String == id)
    }
}

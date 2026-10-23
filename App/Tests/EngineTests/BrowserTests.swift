import Foundation
import Testing

/// The chat's way into the browser, as the Mac's: «open jup.ag» opens the site with the wallet before any model reads
/// the message, and a trade that only starts with «open» doesn't.
@Suite(.serialized) struct BrowserTests {
    /// Sends a message and reads until its answer: the browser's «openSite» if any, and the chat after it.
    static func send(_ engine: EngineTests.Engine, _ text: String, id: Int) throws -> (site: String?, chat: [String]) {
        engine.input.fileHandleForWriting.write(try JSONSerialization.data(withJSONObject: ["id": id, "op": "send", "text": text]) + Data("\n".utf8))
        var site: String?, chat: [String] = [], answered = false
        while let line = try? engine.readLine(timeout: answered ? 1.5 : 30) {
            guard let o = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { continue }
            if o["event"] as? String == "openSite" { site = (o["data"] as? [String: Any])?["url"] as? String }
            if o["event"] as? String == "state" { chat = EngineTests.chat(o["data"] as? [String: Any]) }
            if o["id"] as? Int == id { answered = true }
        }
        return (site, chat)
    }

    @Test func aSiteNamedInTheChatOpensInTheBrowser() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let opened = try Self.send(engine, "open jup.ag", id: 50)
        #expect(opened.site == "jup.ag")
        #expect(opened.chat.last?.contains("jup.ag is open in ihatecrypto's browser") == true, "\(opened.chat)")
        let russian = try Self.send(engine, "открой сайт app.hyperliquid.xyz", id: 51)
        #expect(russian.site == "app.hyperliquid.xyz")
        let trade = try Self.send(engine, "open a long on ETH", id: 52)
        #expect(trade.site == nil)
    }
}

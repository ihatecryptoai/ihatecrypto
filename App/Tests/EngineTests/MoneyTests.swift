@testable import DesktopEngine
import ExecutionCore
import Foundation
import IhatecryptoCore
import Testing

/// Top up's page on 127.0.0.1 and the History page's tabs, from plain inputs.

@Test func aPageRequestIsReadOnlyWhenWhole() {
    let head = "POST /abc/sent?x=1 HTTP/1.1\r\nHost: 127.0.0.1:5000\r\nOrigin: http://127.0.0.1:5000\r\nContent-Length: 8\r\n\r\n"
    #expect(PageRequest.parse(Data("POST /abc HTTP/1.1\r\nHost: x".utf8)) == nil)
    #expect(PageRequest.parse(Data((head + "{\"id\"").utf8)) == nil)
    let r = PageRequest.parse(Data((head + "{\"id\":1}").utf8))
    #expect(r?.method == "POST" && r?.path == "/abc/sent" && r?.headers["origin"] == "http://127.0.0.1:5000" && r?.body == Data("{\"id\":1}".utf8))
}

@Test func onlyThePageItselfIsAnswered() {
    func request(_ method: String, host: String, origin: String? = nil) -> PageRequest {
        PageRequest(method: method, path: "/", headers: ["host": host].merging(origin.map { ["origin": $0] } ?? [:]) { a, _ in a }, body: Data())
    }
    #expect(TopUpPage.allows(request("GET", host: "127.0.0.1:5000"), port: 5000))
    // A name pointed at 127.0.0.1 (DNS rebinding) shows another Host; another site's POST shows another Origin.
    #expect(!TopUpPage.allows(request("GET", host: "evil.example:5000"), port: 5000))
    #expect(!TopUpPage.allows(request("POST", host: "127.0.0.1:5000"), port: 5000))
    #expect(!TopUpPage.allows(request("POST", host: "127.0.0.1:5000", origin: "https://evil.example"), port: 5000))
    #expect(TopUpPage.allows(request("POST", host: "127.0.0.1:5000", origin: "http://127.0.0.1:5000"), port: 5000))
}

@Test func theServerAnswersOnLoopback() async throws {
    let server = try PageServer.start { r in PageServer.response(200, "text/plain", Data("\(r.method) \(r.path) \(r.body.count)".utf8)) }
    defer { server.stop() }
    var post = URLRequest(url: URL(string: "http://127.0.0.1:\(server.port)/page/sent")!)
    post.httpMethod = "POST"
    post.httpBody = Data(repeating: 65, count: 3000)
    let (data, response) = try await URLSession.shared.data(for: post)
    #expect((response as? HTTPURLResponse)?.statusCode == 200)
    #expect(String(decoding: data, as: UTF8.self) == "POST /page/sent 3000")
    #expect((response as? HTTPURLResponse)?.value(forHTTPHeaderField: "X-Frame-Options") == "DENY")
}

@Test func thePageCarriesTheOrderAndNothingCanCloseItsScript() {
    let order = TopUpOrder(network: .solana, coin: .usdc, amount: 20, recipient: "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk")
    let page = TopUpPage.page(order)
    #expect(page.contains("HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk") && page.contains("20 USDC") && !page.contains("{{"))
    #expect(page.contains("\"units\":\"20000000\""))
    #expect(!page.contains("on your Mac"))
}

@Test func usdcToAWalletWithoutSOLBringsItsFeesAlong() throws {
    let order = TopUpOrder(network: .solana, coin: .usdc, amount: 5, recipient: "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk")
    let message = try TopUpPage.usdcWithFees(order, units: 5_000_000, from: "7cVfgArCheMR6Cs4t6vz5rfnqd56vZq4ndaBrY5xkxXy", blockhash: "11111111111111111111111111111111")
    #expect(!message.isEmpty)
}

@Test func historyTabsFollowTheMac() {
    let t0 = Date(timeIntervalSince1970: 1_790_000_000)
    let fills = [
        TradeHistory.Fill(coin: "ETH", px: 3000, sz: Decimal(string: "0.01")!, side: "B", time: t0, startPosition: 0, dir: "Open Long", fee: Decimal(string: "0.02")!),
        TradeHistory.Fill(coin: "ETH", px: 3100, sz: Decimal(string: "0.01")!, side: "A", time: t0.addingTimeInterval(3600), startPosition: Decimal(string: "0.01")!,
                          dir: "Close Long", closedPnl: 1, fee: Decimal(string: "0.02")!, hash: "0xabc"),
    ]
    let asked = HistoryTabs.Activity(kind: "request", at: t0.addingTimeInterval(-60), request: "long eth by the book", understood: "long ETH by the book", coin: "ETH",
                                     lines: ["Opened."])
    let journal = [
        Journal.Entry(at: t0.addingTimeInterval(-90_000), chain: "solana", id: "sig1", kind: .signed, note: "вход"),
        Journal.Entry(at: t0.addingTimeInterval(-89_900), chain: "solana", id: "sig2", kind: .signed, note: "swap"),
        Journal.Entry(at: t0.addingTimeInterval(-80_000), chain: "hyperliquid", id: "n1", kind: .signed, note: "close"),
    ]
    let tabs = Engine.historyTabs(activities: [asked], journal: journal, fills: fills, transfers: [], ru: false)
    #expect(tabs.agent.count == 1 && tabs.agent[0].understood == "long ETH by the book")
    // Two sessions: a pause of over 10 minutes starts a new one; the old Russian note reads in English.
    #expect(tabs.earlier.count == 2 && tabs.earlier[1].bubbles.first?.title == "entry")
    #expect(tabs.stories.count == 1)
    let story = tabs.stories[0]
    #expect(!story.open && story.long && story.realized == Decimal(string: "0.96")! && story.fees == Decimal(string: "0.04")!)
    // The request about ETH a minute before it opened is among the position's events, in time order.
    #expect(story.events.map(\.kind) == ["activity", "open", "close"])
    #expect(tabs.rows.first?.text.hasPrefix("Close Long ETH") == true && tabs.rows.first?.link == "https://app.hyperliquid.xyz/explorer/tx/0xabc")
    #expect(tabs.rows.count == 1 + 2 + 3)
}

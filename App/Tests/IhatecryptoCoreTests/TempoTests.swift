import Foundation
import Testing
@testable import IhatecryptoCore

// A real 402 from CoinGecko's MPP endpoint (2026-09-30): 0.06 USDC on Tempo to Locus's payee.
private let locus = #"Payment id="tLKZnNB4zPmwhEVLFG0tAcnqy81pCHQugJpTRw_w9vk", realm="coingecko.mpp.paywithlocus.com", method="tempo", intent="charge", request="eyJhbW91bnQiOiI2MDAwMCIsImN1cnJlbmN5IjoiMHgyMGMwMDAwMDAwMDAwMDAwMDAwMDAwMDBiOTUzN2QxMWM2MGU4YjUwIiwibWV0aG9kRGV0YWlscyI6eyJjaGFpbklkIjo0MjE3fSwicmVjaXBpZW50IjoiMHgwNjBiMGZCMEJlOWQ5MDU1NzU3N0IzQUVFNDgwNzExMDY3MTQ5RmYwIn0", description="Locus MPP: coingecko/simple-price", expires="2026-09-29T19:32:58.380Z""#
private let before = ISO8601DateFormatter().date(from: "2026-09-29T19:30:00Z")!

/// A challenge with `request` built from this JSON.
private func challenge(_ request: String, extra: String = "") -> String {
    let b64 = Data(request.utf8).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    return #"Bearer realm="x", Payment id="a1", realm="svc", method="tempo", intent="charge", request="\#(b64)""# + extra
}

@Test func payAndTempoCommandsParse() {
    #expect(WalletCommand.parse(#"pay POST https://a.example/v1/search {"q": "flights MOW"}"#)
            == .pay(method: "POST", url: "https://a.example/v1/search", body: #"{"q": "flights MOW"}"#))
    #expect(WalletCommand.parse("pay https://a.example/price") == .pay(method: "GET", url: "https://a.example/price", body: nil))
    #expect(WalletCommand.parse(#"pay https://a.example/x {"a":1}"#) == .pay(method: "POST", url: "https://a.example/x", body: #"{"a":1}"#))
    #expect(WalletCommand.parse("pay http://a.example/x") == nil)   // https only
    #expect(WalletCommand.parse("move 2 USDC from Solana to Tempo") == .solanaTo("tempo", 2))
    #expect(WalletCommand.parse("переведи 3 USDC из соланы в темпо") == .solanaTo("tempo", 3))
    #expect(WalletCommand.parse("move 2 USDC from Tempo to Solana") == .evmToSolana("tempo", 2))
    #expect(WalletCommand.parse("send 5 USDC to Mom") == .sendTo(name: "Mom", amount: 5))
    let pay = #"pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}"#
    #expect(ChatRouter.planChange(pay, markets: ["BTC"]) == nil && ChatRouter.steps(pay) == nil && ScheduledTask.parse(pay) == nil)
    #expect(ChatRouter.parses(pay, markets: ["BTC"]))
    #expect(WalletCommand.placeName("tempo") == "Tempo")
}

@Test func mppChallengeReadsWhatThePayeeAsks() throws {
    let c = try MPP.challenge(locus, now: before).get()
    #expect(c.amount == 60_000 && c.usdc == Decimal(string: "0.06"))
    #expect(c.payee == "0x060b0fB0Be9d90557577B3AEE480711067149Ff0" && c.memo == nil)
    #expect(c.realm == "coingecko.mpp.paywithlocus.com" && c.purpose == "Locus MPP: coingecko/simple-price" && c.field == "Authorization")
    // An expired quote is never paid.
    #expect((try? MPP.challenge(locus, now: before.addingTimeInterval(600)).get()) == nil)

    // The credential echoes the challenge as sent, with the hash and the payer.
    let header = MPP.credential(c, hash: "0xabc", payer: "0x90f79bf6eb2c4f870365e785982e1f101e93b906")
    #expect(header.hasPrefix("Payment ") && !header.contains("=") && !header.contains("+") && !header.contains("/"))
    let json = try #require(MPP.base64url(String(header.dropFirst(8))).flatMap { try JSONSerialization.jsonObject(with: $0) as? [String: Any] })
    let echoed = try #require(json["challenge"] as? [String: String])
    #expect(echoed == ["id": "tLKZnNB4zPmwhEVLFG0tAcnqy81pCHQugJpTRw_w9vk", "realm": "coingecko.mpp.paywithlocus.com", "method": "tempo", "intent": "charge",
                       "request": c.params["request"]!, "description": "Locus MPP: coingecko/simple-price", "expires": "2026-09-29T19:32:58.380Z"])
    #expect(json["payload"] as? [String: String] == ["type": "hash", "hash": "0xabc"])
    #expect(json["source"] as? String == "did:pkh:eip155:4217:0x90f79bf6eb2c4f870365e785982e1f101e93b906")
}

@Test func mppRefusesWhatItCantPay() throws {
    let usdc = #""currency":"0x20c000000000000000000000b9537d11c60e8b50","recipient":"0x060b0fB0Be9d90557577B3AEE480711067149Ff0""#
    func refused(_ request: String, extra: String = "") -> Bool { (try? MPP.challenge(challenge(request, extra: extra)).get()) == nil }
    #expect(!refused(#"{"amount":"1000",\#(usdc)}"#))   // the Bearer scheme beside it is skipped
    #expect(refused(#"{"amount":"0",\#(usdc)}"#))
    #expect(refused(#"{"amount":"1000","currency":"0x20c0000000000000000000000000000000000000","recipient":"0x060b0fB0Be9d90557577B3AEE480711067149Ff0"}"#))
    #expect(refused(#"{"amount":"1000",\#(usdc),"methodDetails":{"chainId":42431}}"#))
    #expect(refused(#"{"amount":"1000",\#(usdc),"methodDetails":{"supportedModes":["pull"]}}"#))
    #expect(!refused(#"{"amount":"1000",\#(usdc),"methodDetails":{"supportedModes":["pull","push"]}}"#))
    #expect(refused(#"{"amount":"1000",\#(usdc),"methodDetails":{"feePayer":true}}"#))
    #expect(refused(#"{"amount":"1000",\#(usdc),"methodDetails":{"splits":[{"recipient":"0x060b0fB0Be9d90557577B3AEE480711067149Ff0","amount":"10"}]}}"#))
    #expect(refused(#"{"amount":"1000",\#(usdc),"methodDetails":{"memo":"0x1234"}}"#))
    let memo = try MPP.challenge(challenge(#"{"amount":"1000",\#(usdc),"methodDetails":{"memo":"0x\#(String(repeating: "ab", count: 32))"}}"#,
                                           extra: #", header="Payment-Authorization""#)).get()
    #expect(memo.memo?.count == 32 && memo.field == "Payment-Authorization")
    #expect((try? MPP.challenge(#"Payment id="a", realm="r", method="stripe", intent="charge", request="e30""#).get()) == nil)
}

@Test func everyMonthOnTheFirst() throws {
    var cal = Calendar(identifier: .gregorian); cal.timeZone = .current
    let now = cal.date(from: DateComponents(year: 2026, month: 9, day: 29, hour: 10, minute: 30))!
    var task = try #require(ScheduledTask.parse("every month on the 1st at 10:00: send 20 USDC to Mom", now: now))
    #expect(task.steps == ["send 20 USDC to Mom"] && task.every == .months(1))
    #expect(task.next == cal.date(from: DateComponents(year: 2026, month: 10, day: 1, hour: 10)))
    #expect(task.advance(now: task.next) && task.next == cal.date(from: DateComponents(year: 2026, month: 11, day: 1, hour: 10)))
    #expect(ScheduledTask.parse("every month on day 15: rebalance USDC", now: now)?.next == cal.date(from: DateComponents(year: 2026, month: 10, day: 15, hour: 9)))
    #expect(ScheduledTask.parse("every month on the 31st: rebalance USDC", now: now) == nil)   // not every month has it
}

@Test func tempoIsAModule() {
    #expect(ModuleNetwork(place: "tempo") == .tempo && ModuleNetwork.tempo.logo == "tempo")
    var m = Modules()
    m.set(.tempo, false)
    #expect(m.refusal(.pay(method: "GET", url: "https://a.example", body: nil)) != nil)
    #expect(m.refusal(.solanaTo("tempo", 2)) != nil && m.refusal(.solanaTo("base", 2)) == nil)
    // Payments have their own switch: off, «pay …» is refused while Tempo transfers still go.
    var p = Modules()
    p.set(.mpp, false)
    #expect(p.refusal(.pay(method: "GET", url: "https://a.example", body: nil)) != nil && p.refusal(.solanaTo("tempo", 2)) == nil)
    #expect(ModuleProtocol.mpp.kind == .payments && ModuleProtocol.mpp.networks == [.tempo])
}

@Test func mppDirectoryRanksByName() {
    let fax: [String: Any] = ["id": "agentfax", "name": "agentfax", "description": "Send a fax, priced per page. No API key.", "tags": ["fax"], "categories": ["social"]]
    let gecko: [String: Any] = ["id": "coingecko", "name": "CoinGecko", "description": "Cryptocurrency market data — prices, charts.", "tags": ["crypto", "prices"],
                                "categories": ["data"], "endpoints": [["description": "Simple Price"]]]
    let ids = { (q: String) in MPP.rank([fax, gecko], q).map { $0["id"] as? String ?? "" } }
    #expect(ids("BTC price USD").first == "coingecko" && ids("coingecko") == ["coingecko"] && ids("fax") == ["agentfax"])
    #expect(ids("paid api service").count == 2)   // nothing but service words: everything, unranked
}

// A service's answer on the canvas: Brave's hits as rows wherever they sit, <strong> gone; plain values otherwise.
@Test func serviceAnswerReadsResultsAndValues() {
    let brave = #"{"success":true,"data":{"type":"search","query":{"original":"memorycowmoo"},"web":{"results":[{"title":"<strong>Memory</strong> cow Moo","url":"https://x.com/memorycowmoo","description":"Moo &amp; more"},{"title":"Second","url":"https://example.com/2"}]}}}"#
    let a = MPP.Answer(Data(brave.utf8))
    #expect(a.items == [.init(title: "Memory cow Moo", url: "https://x.com/memorycowmoo", detail: "Moo & more"), .init(title: "Second", url: "https://example.com/2", detail: nil)])
    let price = MPP.Answer(Data(#"{"bitcoin":{"usd":83570.5}}"#.utf8))
    #expect(price.items.isEmpty && price.facts == [.init(key: "bitcoin.usd", value: "83570.5")])
    #expect(MPP.Answer(Data("queued".utf8)).text == "queued")
    #expect(MPP.Answer(Data(#"{"images":[{"url":"https://cdn.example/a.png","width":1024}]}"#.utf8)).items.first?.isImage == true)
}

// The chat folds a service's JSON under a reply, whole or cut short (the first paid answer's was cut at 1,500 characters).
@Test func chatFoldsServiceJSON() {
    let old = "Paid 0.035 USDC to brave.mpp.paywithlocus.com on Tempo. https://explore.tempo.xyz/tx/0xab\n\n{\n  \"success\" : true,\n  \"data\" : {\n    \"type\" : \"search\",\n    \"query\" : {\n      \"original\" : \"site:x.com (\\\"memorycowmoo\\\")\",\n      \"more_results_available\" : false"
    let (words, data) = MPP.splitData(old)
    #expect(words == "Paid 0.035 USDC to brave.mpp.paywithlocus.com on Tempo. https://explore.tempo.xyz/tx/0xab" && data?.hasPrefix("{") == true)
    #expect(MPP.rows(data ?? "").first?.title == "type: search")
    let hits = #"{"web":{"results":[{"title":"<strong>Moo</strong>","url":"https://x.com/moo"},{"title":"Two","url":"https://b.example"}]}}"#
    #expect(MPP.rows(hits).map(\.title) == ["Moo", "Two"] && MPP.rows(hits).first?.url == "https://x.com/moo")
    #expect(MPP.splitData("Plain words.\n\nNo data here.").1 == nil)
}

// A service's docs from the section of the endpoint asked for, not the doc's start.
@Test func serviceDocsStartAtTheEndpoint() {
    let docs = "# Grok\nintro\n### Chat\n/grok/chat body\n### Chat with X Search\ncurl -X POST https://grok.example/grok/x-search -d '{\"model\":\"grok-4.6\"}'"
    #expect(MPP.section(docs, about: ["/grok/x-search"]).hasPrefix("\n### Chat with X Search"))
    #expect(MPP.section(docs, about: []).hasPrefix("# Grok"))
}

// An AI service's answer (xAI's Responses API through Locus): its words and citations, not its tool calls; the chat's model
// reads that text, not the JSON's start (the first Grok answer about MOO came out «cut» and empty).
@Test func aiServiceAnswerIsItsWriting() throws {
    let text = "MOO fell about 26% today as early holders sold into thin liquidity; posts on X are mostly worried, a few call it a dip to buy."
    let grok = """
    {"success":true,"data":{"id":"resp_1","output":[{"id":"rs_1","type":"reasoning","status":"completed","encrypted_content":"\(String(repeating: "QUJD", count: 400))"},
    {"type":"custom_tool_call","name":"x_keyword_search","call_id":"xs_1","input":"{\\"query\\":\\"memorycowmoo\\"}"},
    {"type":"message","role":"assistant","content":[{"type":"output_text","text":"\(text)","annotations":[{"type":"url_citation","url":"https://x.com/a/status/1"}]}]}],
    "citations":["https://x.com/a/status/1","https://x.com/b/status/2"]}}
    """
    let a = MPP.Answer(Data(grok.utf8))
    #expect(a.writing == text && a.links == ["https://x.com/a/status/1", "https://x.com/b/status/2"] && a.items.isEmpty)
    let digest = MPP.digest(Data(grok.utf8))
    #expect(digest.contains(text) && digest.contains("https://x.com/b/status/2") && !digest.contains("QUJD"))
}

// A paid answer is kept with its move in the canvas file: it survives the trip through JSON.
@Test func serviceAnswerSurvivesSaving() throws {
    let a = MPP.Answer(Data(#"{"output":[{"type":"message","content":[{"type":"output_text","text":"MOO fell as early holders sold into thin liquidity; the mood on X is mostly worried."}]}],"citations":["https://x.com/a/status/1"]}"#.utf8))
    #expect(try JSONDecoder().decode(MPP.Answer.self, from: JSONEncoder().encode(a)) == a && a.writing != nil)
}

// Between two EVM networks straight, not through Solana: the user's «из бейза на робингуд».
@Test func evmToEvmMovesParse() {
    #expect(WalletCommand.parse("move 20 USDC from Base to Robinhood") == .bridge(from: "base", to: "robinhood", 20))
    #expect(WalletCommand.parse("переведи 20 USDC из бейза в робингуд") == .bridge(from: "base", to: "robinhood", 20))
    #expect(WalletCommand.parse("move all USDC from Arbitrum to Tempo") == .bridge(from: "arbitrum", to: "tempo", nil))
    #expect(WalletCommand.parse("move 20 USDC from Base to Hyperliquid") == .bridge(from: "base", to: "hyperliquid", 20))
    #expect(WalletCommand.parse("переведи 20 USDC с бейза на гипер") == .bridge(from: "base", to: "hyperliquid", 20))
    #expect(WalletCommand.parse("move 20 USDC from Base to Solana") == .baseToSolana(20))
    #expect(WalletCommand.parse("move 5 USDC from Solana to Base") == .solanaTo("base", 5))
    #expect(WalletCommand.parse("withdraw USDC from Aave on Arbitrum") != .bridge(from: "arbitrum", to: "arbitrum", nil))
    var m = Modules()
    m.set(.robinhood, false)
    #expect(m.refusal(.bridge(from: "base", to: "robinhood", 5)) != nil && Modules().refusal(.bridge(from: "base", to: "robinhood", 5)) == nil)
}

// Gas is never a dead end: «добавь газа на робингуд на 3 бакса» is a command, not «ihatecrypto can't».
@Test func gasTopUpsParse() {
    #expect(WalletCommand.parse("добавь газа на робингуд на 3 бакса") == .gas("robinhood", usd: 3))
    #expect(WalletCommand.parse("add gas on base for 2 USDC") == .gas("base", usd: 2))
    #expect(WalletCommand.parse("top up gas on arbitrum") == .gas("arbitrum", usd: nil))
    #expect(WalletCommand.parse("докинь газ в полигон") == .gas("polygon", usd: nil))
    #expect(WalletCommand.parse("move 20 USDC from Base to Robinhood") == .bridge(from: "base", to: "robinhood", 20))
    #expect(ChatRouter.parses("add gas on robinhood for 1 USDC", markets: []))
}

/// Services that took the test wallet's payment and refused its proof (2026-10-23) stay out of the model's search.
@Test func servicesThatRefuseTheProofAreNeverOffered() {
    #expect(MPP.unpayable(["serviceUrl": "https://api.aisa.one"]))
    #expect(MPP.unpayable(["serviceUrl": "https://mpp.alchemy.com"]))
    #expect(MPP.unpayable(["serviceUrl": "https://stableenrich.dev"]))
    #expect(!MPP.unpayable(["serviceUrl": "https://brave.mpp.paywithlocus.com"]))
}

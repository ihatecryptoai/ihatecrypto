import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
@testable import OneClickKit
import RelayBridge
import SolanaChain
import Testing
@testable import TradeExecution
import WalletCore

/// The partner key goes on every 1Click request as X-API-Key, and only when there is one.
@Test func oneClickSendsThePartnerKeyOnlyWhenSet() {
    let url = URL(string: "https://1click.chaindefuser.com/v0/status?depositAddress=x")!
    OneClick.key = nil
    #expect(OneClick.request(url).value(forHTTPHeaderField: "X-API-Key") == nil)
    OneClick.key = "eyJ.test.key"
    #expect(OneClick.request(url).value(forHTTPHeaderField: "X-API-Key") == "eyJ.test.key")
    OneClick.key = ""
    #expect(OneClick.key == nil && OneClick.request(url).value(forHTTPHeaderField: "X-API-Key") == nil)
}

/// Live (IHATECRYPTO_NET=1, a dry quote, no money): 1Click turns a key it doesn't know away with 401, which Settings says in
/// words; without a key the same quote still answers.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func oneClickTurnsAnUnknownKeyAway() async throws {
    let why = await OneClick.problem(withKey: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.abc")
    print("bad key:", why ?? "accepted?")
    #expect(why?.contains("turned the key away (401)") == true)
    OneClick.key = nil
    let q = try await OneClick.quote(dry: true, origin: OneClick.Asset.solanaUSDC, destination: OneClick.Asset.near, amount: "5000000",
                                     refundTo: "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm", recipient: String(repeating: "ab", count: 32))   // an implicit NEAR account, as the app's
    print("5 USDC → NEAR without a key:", q.amountOutFormatted)
    #expect(Decimal(string: q.amountOutFormatted) ?? 0 > 0)
}

/// Every state 1Click reports reads in words on the running card.
@Test func oneClickStatesReadInWords() {
    for state in ["PENDING_DEPOSIT", "KNOWN_DEPOSIT_TX", "INCOMPLETE_DEPOSIT", "PROCESSING", "SUCCESS", "REFUNDED", "FAILED"] {
        #expect(!(Executor.oneClickWords[state] ?? "").isEmpty, "\(state)")
    }
}

/// Live (IHATECRYPTO_NET=1, dry, no money): 1Click quotes NEAR into USDC at an EVM address on Base, Arbitrum and Ethereum.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func oneClickPaysNearSalesOnEVMNetworks() async throws {
    for net in ["base", "arbitrum", "ethereum"] {
        let q = try await OneClick.quote(dry: true, origin: OneClick.Asset.near, destination: try #require(OneClick.Asset.usdc(onEVM: net)),
                                         amount: "1000000000000000000000000", refundTo: String(repeating: "ab", count: 32),
                                         recipient: "0x00000000000000000000000000000000000f0a11")
        print("1 NEAR → USDC on \(net): \(q.amountOutFormatted), ≈ \(q.timeEstimate ?? -1) s")
        #expect(Decimal(string: q.amountOutFormatted) ?? 0 > 0)
    }
}

/// 1Click's USDC on each place the bridges compare it on: Solana, Base, Arbitrum, Ethereum — not Polygon or Robinhood.
@Test func oneClickUSDCOnTheBridgedPlaces() {
    #expect(Executor.oneClickUSDC(nil) == OneClick.Asset.solanaUSDC)
    for net in ["base", "arbitrum", "ethereum"] { #expect(Executor.oneClickUSDC(net) == OneClick.Asset.usdc(onEVM: net)) }
    // A new Hyperliquid account opened through 1Click lands on the perp balance.
    #expect(Executor.oneClickUSDC("hyperliquid") == OneClick.Asset.hyperCoreUSDC)
    #expect(Executor.oneClickUSDC("polygon") == nil && Executor.oneClickUSDC("robinhood") == nil)
}

/// Live (IHATECRYPTO_NET=1, dry quotes, no money): Relay and 1Click quoted for the same USDC moves, the one the bridge
/// would take printed — each quotes every pair it serves.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func relayAndOneClickQuoteTheSameMoves() async throws {
    let me = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", sol = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm"
    let e = Executor(wallet: TradeWallet(solana: sol, evm: me), signer: OneClickQuoteOnly(),
                     journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("oc-\(UUID().uuidString).jsonl"))) { _ in }
    for (from, to, units) in [("base", nil, UInt64(5_000_000)), (nil, "arbitrum", 5_000_000), ("arbitrum", "base", 20_000_000), ("base", nil, 200_000_000)] as [(String?, String?, UInt64)] {
        let oc = await e.oneClickQuote(from: from, to: to, units: units)
        let fromNet = from.flatMap(EVMNetwork.named), toNet = to.flatMap(EVMNetwork.named)
        let relay = try? await Relay.quote(user: from == nil ? sol : me, recipient: to == nil ? sol : me,
                                           originChain: fromNet?.relayChain ?? Relay.Chain.solana, destinationChain: toNet?.relayChain ?? Relay.Chain.solana,
                                           originCurrency: fromNet?.usdc ?? Relay.Currency.solanaUSDC, destinationCurrency: toNet?.usdc ?? Relay.Currency.solanaUSDC, amount: units)
        print("\(Decimal(units) / 1_000_000) USDC \(from ?? "solana") → \(to ?? "solana"): Relay \(relay?.amountOutFormatted ?? "—") · 1Click \(oc?.amountOutFormatted ?? "—") (≈ \(oc?.timeEstimate ?? -1) s)")
        #expect(oc != nil && relay != nil)
    }
}

private struct OneClickQuoteOnly: TradeSigner {
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError("no") }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw ExecutionError("no") }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data { throw ExecutionError("no") }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError("no") }
}

/// 1Click is taken over Relay only for an edge worth another Touch ID: 10 cents, or 0.05% of a bigger move.
@Test func oneClickNeedsAnEdgeOverRelay() {
    #expect(Executor.oneClickEdge(5) == Decimal(string: "0.10")!)
    #expect(Executor.oneClickEdge(200) == Decimal(string: "0.10")!)
    #expect(Executor.oneClickEdge(1000) == Decimal(string: "0.5")!)
}

/// Money in transit: bridge orders (Relay's, 1Click's) signed and not settled, within a day, newest first; swaps and
/// settled or failed orders are not.
@Test func journalKnowsWhatIsInTransit() throws {
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("transit-\(UUID().uuidString).jsonl"))
    let now = Date()
    try journal.append(.init(at: now.addingTimeInterval(-600), chain: "relay", id: "r1", kind: .signed, note: "Base USDC→Solana 5, tx 0xabc"))
    try journal.append(.init(at: now.addingTimeInterval(-500), chain: "relay", id: "r2", kind: .signed, note: "Arbitrum USDC→Base 3"))
    try journal.append(.init(at: now.addingTimeInterval(-400), chain: "relay", id: "r2", kind: .settled, note: "success"))
    try journal.append(.init(at: now.addingTimeInterval(-300), chain: "1click", id: "dep", kind: .signed, note: "USDC→NEAR 1.5 from Solana"))
    try journal.append(.init(at: now.addingTimeInterval(-200), chain: "base", id: "0xswap", kind: .signed, note: "Swap"))
    try journal.append(.init(at: now.addingTimeInterval(-90_000), chain: "relay", id: "old", kind: .signed, note: "two days ago"))
    #expect(try journal.inTransit(now: now).map(\.id) == ["dep", "r1"])
}

/// Live (IHATECRYPTO_NET=1, a scratch journal): an order the app never heard the end of — a Relay swap that settled on
/// 2026-10-04 — is asked of Relay, its end written, and it leaves «in transit»; a fresh one stays.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func transitResolvesAnOrderThatLandedMeanwhile() async throws {
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("resolve-\(UUID().uuidString).jsonl"))
    let now = Date()
    try journal.append(.init(at: now.addingTimeInterval(-3_600), chain: "relay", id: "0x1791142867bd8d3585900c66c5bb90ed01078e0281434af0e9c15e3dee99ef30", kind: .signed, note: "Robinhood USDG→AI"))
    try journal.append(.init(at: now.addingTimeInterval(-30), chain: "relay", id: "0xfresh", kind: .signed, note: "Base USDC→Solana 1"))
    let e = Executor(wallet: TradeWallet(solana: "", evm: ""), signer: OneClickQuoteOnly(), journal: journal) { _ in }
    let left = await e.resolveTransit(now: now)
    #expect(left.map(\.id) == ["0xfresh"])
    #expect(try journal.entries().last?.kind == .settled)
}

/// A spot token's USDC pair from spotMeta's lists: asset 10000 + the pair's index, its key in allMids, its size decimals.
@Test func hyperliquidSpotPairFromItsLists() throws {
    let tokens: [[String: Any]] = [["name": "USDC", "index": 0, "szDecimals": 8], ["name": "PURR", "index": 1, "szDecimals": 0], ["name": "HYPE", "index": 150, "szDecimals": 2]]
    let universe: [[String: Any]] = [["tokens": [1, 0], "index": 0, "name": "PURR/USDC"], ["tokens": [150, 0], "index": 107, "name": "@107"]]
    #expect(try Executor.spotPair("hype", tokens: tokens, universe: universe) == Executor.SpotPair(asset: 10_107, key: "@107", symbol: "HYPE", szDecimals: 2))
    #expect(try Executor.spotPair("PURR", tokens: tokens, universe: universe).asset == 10_000)
    #expect(throws: (any Error).self) { try Executor.spotPair("DOGE", tokens: tokens, universe: universe) }
}

extension FakeHyperliquid {
    /// A spot swap finds both pairs before the sale: a `to` the book can't buy (USDC itself, «USDC.e») or `from` again
    /// (USDT is USDT0 there) stops with nothing signed or sent, so Try again can't sell twice. A pair the book has goes on.
    @Test func spotSwapChecksBothPairsBeforeTheSale() async throws {
        URLProtocol.registerClass(FakeHL.self)
        defer { URLProtocol.unregisterClass(FakeHL.self) }
        FakeHL.lock.withLock { FakeHL.state = .init() }
        let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("spot-\(UUID().uuidString).jsonl"))
        let e = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(), journal: journal) { _ in }
        for (from, to, why) in [("HYPE", "USDC", "USDC has no USDC pair"), ("HYPE", "USDC.e", "USDC.E has no USDC pair"),
                                ("HYPE", "hype", "HYPE for HYPE: the same token"), ("USDT", "USDT0", "USDT0 for USDT0: the same token")] {
            let error = await #expect(throws: ExecutionError.self, "\(from) → \(to)") { try await e.hlSpotSwap(from: from, to: to, amount: 1) }
            #expect(error?.message.contains(why) == true && error?.message.hasSuffix("Nothing was signed.") == true, "\(error?.message ?? "")")
        }
        // HYPE for PURR passes the check and goes on to the spot balance, which this Hyperliquid doesn't answer.
        let next = await #expect(throws: ExecutionError.self) { try await e.hlSpotSwap(from: "HYPE", to: "PURR", amount: 1) }
        #expect(next?.message == "Hyperliquid's spot balance didn't come.")
        #expect(FakeHL.lock.withLock { FakeHL.state.sent } == [])
        #expect(try journal.entries().isEmpty)
    }
}

/// Live (IHATECRYPTO_NET=1, reads only): HYPE's USDC pair and its price on Hyperliquid's spot book now; USDT is USDT0
/// there, and neither USDC nor «USDC.e» has a USDC pair to buy.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func hyperliquidSpotHasHYPE() async throws {
    let pair = try await Executor.spotPair("HYPE"), mid = try await Executor.spotMid(pair)
    print("HYPE spot: asset \(pair.asset), \(pair.key), szDecimals \(pair.szDecimals), mid \(mid)")
    #expect(pair.asset >= 10_000 && mid > 1)
    #expect(try await Executor.spotPair("USDT").asset == (try await Executor.spotPair("USDT0")).asset)
    for none in ["USDC", "USDC.E"] { await #expect(throws: ExecutionError.self, "\(none)") { try await Executor.spotPair(none) } }
}

/// Live (IHATECRYPTO_NET=1, quotes only): Relay's cross-network swaps for the test wallet — SOL on Solana for ETH on
/// Arbitrum, and back — with the checks that run before anything is signed.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func crossNetworkSwapsAreQuoted() async throws {
    let e = Executor(wallet: TradeWallet(solana: "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm", evm: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"),
                     signer: OneClickQuoteOnly(), journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("x\(UUID()).jsonl"))) { _ in }
    let there = try await e.crossQuoteText("SOL", on: "solana", for: "ETH", on: "arbitrum", amount: Decimal(string: "0.01")!)
    print("0.01 SOL → Arbitrum:", there)
    #expect(there.gets.contains("ETH"))
    // All of a coin is never asked: its fees come out of it.
    await #expect(throws: (any Error).self) { try await e.crossQuoteText("SOL", on: "solana", for: "ETH", on: "base", amount: nil) }
}

/// A Solana node holding 10 USDC for the wallet («cross.stub») and Relay quoting them for $8 of ETH: a $2 loss of $10,
/// past the cap of max(3%, 15¢). It counts the balance reads and keeps the amounts Relay was asked for; `worthIn` is
/// Relay's dollar price of what goes in.
private final class CrossStub: URLProtocol {
    nonisolated(unsafe) static var balanceReads = 0
    nonisolated(unsafe) static var relayAmounts: [String] = []
    nonisolated(unsafe) static var worthIn = "10"
    static let sol = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm", evm = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"

    override class func canInit(with request: URLRequest) -> Bool {
        request.url?.host == "cross.stub" || (request.url?.host == "api.relay.link" && request.url?.path == "/quote/v2")
    }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var body = Data()
        if let stream = request.httpBodyStream {   // URLProtocol gets the body as a stream, not httpBody
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 { body.append(buffer, count: n) }
            stream.close()
        }
        let j = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]
        let reply: [String: Any]
        if request.url?.host == "cross.stub" {
            Self.balanceReads += 1
            let held: [String: Any] = ["account": ["data": ["parsed": ["info": ["tokenAmount": ["amount": "10000000"]]]]]]
            reply = ["jsonrpc": "2.0", "id": 1, "result": ["value": [held]]]
        } else {
            Self.relayAmounts.append(j["amount"] as? String ?? "")
            reply = ["requestId": "0xstub", "steps": [] as [Any],
                     "details": ["sender": Self.sol, "recipient": Self.evm, "timeEstimate": 3,
                                 "currencyIn": ["amount": "10000000", "amountFormatted": "10", "amountUsd": Self.worthIn],
                                 "currencyOut": ["amount": "2000000000000000", "amountFormatted": "0.002", "amountUsd": "8"]]]
        }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: try! JSONSerialization.data(withJSONObject: reply))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// The tests answered by CrossStub, one at a time: its counters and its price are shared. Off with IHATECRYPTO_NET=1: the
/// live tests ask the real Relay while the stub is in.
@Suite(.serialized, .disabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
struct CrossStubbed {
    func executor() -> Executor {
        Executor(wallet: TradeWallet(solana: CrossStub.sol, evm: CrossStub.evm), signer: OneClickQuoteOnly(),
                 journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("cs-\(UUID().uuidString).jsonl")),
                 rpc: SolanaRPC(url: URL(string: "https://cross.stub")!)) { _ in }
    }

    /// «All» of a token on the card: Relay's quote shows even past the loss cap, from one balance read and one quote of
    /// exactly what's held; the run still stops at the cap (CostlyMove, «Send anyway»).
    @Test func crossCardQuotesAllOfATokenPastTheLossCap() async throws {
        URLProtocol.registerClass(CrossStub.self)
        defer { URLProtocol.unregisterClass(CrossStub.self) }
        (CrossStub.balanceReads, CrossStub.relayAmounts, CrossStub.worthIn) = (0, [], "10")
        let e = executor()
        let card = try await e.crossQuoteText("USDC", on: "solana", for: "ETH", on: "arbitrum", amount: nil)
        #expect(card.gets.contains("0.002 ETH") && card.gets.contains("$8"))
        #expect(CrossStub.balanceReads == 1 && CrossStub.relayAmounts == ["10000000"])
        await #expect(throws: CostlyMove.self) {
            try await e.crossQuote("USDC", on: "solana", for: "ETH", on: "arbitrum", amount: nil)
        }
    }

    /// Relay pricing what goes in at nothing (no price, «0», «NaN») stops the swap before signing: read as $0, the loss
    /// check let any loss through. The card stops too, so it shows the way Confirm takes. With a price it checks as before.
    @Test func crossQuoteWithoutAPriceStops() async throws {
        URLProtocol.registerClass(CrossStub.self)
        defer { URLProtocol.unregisterClass(CrossStub.self); CrossStub.worthIn = "10" }
        let e = executor()
        func quote(_ worthIn: String, capped: Bool = true) async throws {
            CrossStub.worthIn = worthIn
            _ = try await e.crossQuote("USDC", on: "solana", for: "ETH", on: "arbitrum", amount: 10, capped: capped)
        }
        for none in ["", "0", "NaN", "-1"] {
            for capped in [true, false] {
                let error = await #expect(throws: ExecutionError.self, "worth in «\(none)»") { try await quote(none, capped: capped) }
                #expect(error?.message.contains("can't be checked") == true)
            }
        }
        try await quote("8.2")                                              // $0.20 of $8.20 lost, under 3%: passes
        await #expect(throws: CostlyMove.self) { try await quote("10") }    // $2 of $10: asks «Send anyway»
        // 1Click pricing only what goes in: refused, never asked about as a 100% loss.
        #expect(throws: ExecutionError.self) { try Executor.holdsCrossLoss(in: 10, out: 0, unpriced: "1Click doesn't price X for Y", what: "NEAR Intents") }
    }
}

/// Live (IHATECRYPTO_NET=1, dry quotes only): what 0.0001 BTC to a real Bitcoin address and 20 USDT on TRON cost in
/// USDC from the test wallet, and that a coin 1Click doesn't pay there is refused.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func sendsAcrossAreQuoted() async throws {
    let e = Executor(wallet: TradeWallet(solana: "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm", evm: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"),
                     signer: OneClickQuoteOnly(), journal: Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("a\(UUID()).jsonl"))) { _ in }
    let btc = try await e.acrossQuote("BTC", amount: Decimal(string: "0.0001")!, network: "bitcoin", to: "bc1quc8z9y2dmpe2qztdw4zh0ucr7g5p8gw6x3c8ph")
    let usdt = try await e.acrossQuote("USDT", amount: 20, network: "tron", to: "TNXoiAJ3dct8Fjg4M9fkLFh9S2v9TXc32G")
    print("0.0001 BTC costs \(btc.quote.amountInFormatted) USDC from \(btc.source?.name ?? "Solana") in ≈ \(btc.quote.timeEstimate ?? 0) s; 20 USDT on TRON costs \(usdt.quote.amountInFormatted)")
    #expect((Decimal(string: btc.quote.amountInFormatted) ?? 0) > 1 && (Decimal(string: usdt.quote.amountInFormatted) ?? 0) >= 20)
    await #expect(throws: (any Error).self) { try await e.acrossQuote("ETH", amount: 1, network: "bitcoin", to: "bc1quc8z9y2dmpe2qztdw4zh0ucr7g5p8gw6x3c8ph") }
}

/// Live (IHATECRYPTO_NET=1, quotes only): Tempo's own exchange prices USDC → pathUSD and → OUSD within a few basis points.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func tempoExchangeQuotes() async throws {
    let rpc = EVMRPC.tempo
    for to in ["PATHUSD", "OUSD"] {
        let out = ABI.uint(in: try await rpc.read(Tempo.exchange, try Tempo.quote(Tempo.stables["USDC"]!, Tempo.stables[to]!, units: 10_000_000)), word: 0) ?? 0
        print("Tempo exchange: 10 USDC → \(Decimal(out) / 1_000_000) \(to)")
        #expect(out > 9_900_000 && out <= 10_000_000)
    }
    #expect(try Tempo.swap(Tempo.stables["USDC"]!, Tempo.stables["OUSD"]!, units: 1, minOut: 1).prefix(4).hex == "f8856c0f")
}

/// A chain's bridge legs land together: waiting for one place leaves the others running, they overlap in time, and a
/// leg that failed throws when its place is needed.
@Test(.timeLimit(.minutes(1))) func landingsAreWaitedForByPlace() async throws {
    let landings = Landings()
    let started = ContinuousClock.now
    landings.add("arbitrum") { try await Task.sleep(for: .milliseconds(400)); return "one" }
    landings.add("arbitrum") { try await Task.sleep(for: .milliseconds(400)); return "two" }
    landings.add("solana") { throw ExecutionError("Relay returned refund.") }
    #expect(landings.count(at: "arbitrum") == 2 && landings.count(at: nil) == 3)
    #expect(try await landings.wait(for: "arbitrum") == ["one", "two"])
    // Two 0.4 s landings at once, not one after the other.
    #expect(ContinuousClock.now - started < .milliseconds(750))
    #expect(landings.count(at: "arbitrum") == 0 && landings.count(at: "solana") == 1)
    await #expect(throws: ExecutionError.self) { try await landings.wait(for: "solana") }
    #expect(landings.count(at: nil) == 0)
}

/// A settled 1Click swap as the NEAR chart's cost reads it (the shape of /status for the 1,000 USDC buy on 2026-10-10):
/// NEAR in for USDC costs the USDC, NEAR in for another coin its dollar value, NEAR out is a sale, someone else's is none.
@Test func oneClickSettledSwapsMoveNEAR() {
    func status(_ origin: String, _ destination: String, recipient: String, refundTo: String, state: String = "SUCCESS") -> [String: Any] {
        ["status": state,
         "swapDetails": ["amountInFormatted": "1000.0", "amountInUsd": "999.75", "amountOutFormatted": "185.421463672726355985056693"],
         "quoteResponse": ["quoteRequest": ["originAsset": origin, "destinationAsset": destination, "recipient": recipient, "refundTo": refundTo]]]
    }
    let me = "8ec67b662faddd44e7d79875a4cf1e7f8c3f756a4b171ccea95a4149b8dddf2b", near = OneClick.Asset.near
    let buy = OneClick.Settled(status(OneClick.Asset.solanaUSDC, near, recipient: me, refundTo: "AULW53"))
    #expect(buy?.near(for: me)?.amount == Decimal(string: "185.421463672726355985056693") && buy?.near(for: me)?.dollars == 1000)
    #expect(OneClick.Settled(status(OneClick.Asset.solanaSOL, near, recipient: me, refundTo: "AULW53"))?.near(for: me)?.dollars == Decimal(string: "999.75"))
    #expect(OneClick.Settled(status(near, OneClick.Asset.solanaUSDC, recipient: "AULW53", refundTo: me))?.near(for: me)?.amount == -1000)
    #expect(buy?.near(for: "d523049b8db090f8b47bdbfc027ba4c42df08782835303be44ea61d9f1955dfb") == nil)
    #expect(OneClick.Settled(status(OneClick.Asset.solanaUSDC, near, recipient: me, refundTo: "AULW53", state: "REFUNDED")) == nil)
}

/// 1Click's /status for the deposit addresses «stub-<case>», as it answers them; every other request goes out as usual.
private final class StatusStub: URLProtocol {
    static let installed = URLProtocol.registerClass(StatusStub.self)
    static let answers: [String: (Int, String)] = [
        "stub-ok": (200, #"{"status":"SUCCESS","swapDetails":{"amountInFormatted":"1000.0","amountInUsd":"999.75","amountOutFormatted":"185.42"},"quoteResponse":{"quoteRequest":{"originAsset":"\#(OneClick.Asset.solanaUSDC)","destinationAsset":"\#(OneClick.Asset.near)","recipient":"me.near","refundTo":"AULW53"}}}"#),
        "stub-pending": (200, #"{"status":"PROCESSING"}"#),
        "stub-unknown": (404, #"{"message":"Deposit address stub-unknown not found","error":"Not Found","statusCode":404}"#),
        "stub-down": (500, #"{"statusCode":500,"message":"Internal server error"}"#),
        "stub-busy": (429, #"{"statusCode":429,"message":"ThrottlerException: Too Many Requests"}"#),
        "stub-page": (502, "<html><body>502 Bad Gateway</body></html>"),
        "stub-empty": (200, "{}"),
        "stub-gateway": (404, "<html><body>404 Not Found</body></html>"),
    ]
    /// «stub-flaky»: throttled once, then the swap.
    nonisolated(unsafe) static var flaky = 0
    static func address(_ r: URLRequest) -> String? {
        guard r.url?.host == "1click.chaindefuser.com", r.url?.path.hasSuffix("/status") == true else { return nil }
        return URLComponents(url: r.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "depositAddress" }?.value
    }
    override class func canInit(with request: URLRequest) -> Bool { address(request)?.hasPrefix("stub-") == true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var address = Self.address(request) ?? ""
        if address == "stub-flaky" { Self.flaky += 1; address = Self.flaky == 1 ? "stub-busy" : "stub-ok" }
        let (code, body) = Self.answers[address] ?? (404, "{}")
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: code, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(body.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// NEAR's cost reads every 1Click swap's /status: a swap 1Click didn't answer for dropped out, and the cost without it
/// (its NEAR counted as held before, at that day's price) was kept till the amount changed. 1Click sends its failures as
/// JSON (NestJS), so an error code, a page (a gateway's 404 too) or JSON with no status is no answer (throws); a swap
/// that isn't a success, or an address it doesn't know (its own 404), is one (nil). A throttle is asked again first.
@Test func oneClickSettledTellsNoAnswerFromNoSwap() async throws {
    _ = StatusStub.installed
    #expect(try await OneClick.settled(depositAddress: "stub-ok")?.near(for: "me.near")?.amount == Decimal(string: "185.42"))
    #expect(try await OneClick.settled(depositAddress: "stub-pending") == nil)
    #expect(try await OneClick.settled(depositAddress: "stub-unknown") == nil)
    for down in ["stub-down", "stub-busy", "stub-page", "stub-empty", "stub-gateway"] {
        await #expect(throws: (any Error).self, "\(down)") { try await OneClick.settled(depositAddress: down, delays: []) }
    }
    #expect(try await OneClick.settled(depositAddress: "stub-flaky", delays: [.milliseconds(1)]) != nil && StatusStub.flaky == 2)
}

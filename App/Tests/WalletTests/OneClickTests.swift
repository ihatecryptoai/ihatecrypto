import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
@testable import OneClickKit
import RelayBridge
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

/// Live (IHATECRYPTO_NET=1, reads only): HYPE's USDC pair and its price on Hyperliquid's spot book now.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func hyperliquidSpotHasHYPE() async throws {
    let pair = try await Executor.spotPair("HYPE"), mid = try await Executor.spotMid(pair)
    print("HYPE spot: asset \(pair.asset), \(pair.key), szDecimals \(pair.szDecimals), mid \(mid)")
    #expect(pair.asset >= 10_000 && mid > 1)
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

import ExecutionCore
import Foundation
@testable import SolanaChain
import Testing
@testable import TradeExecution
import WalletCore

// A swap we sign second (Jupiter's gasless swap, a JupiterZ fill) that Jupiter's /execute didn't confirm: the chain is
// asked what became of it. The network knows it only by the sponsor's signature, the first, so it's looked for among the
// wallet's own transactions; «expired» (which the chat offers to try again) only when the chain proves it can't land.

/// A Solana node at https://<name>.cosigned.stub, the way a real one answers: `minContextSlot` past its own slot is
/// refused («Minimum context slot has not been reached»), so a node behind proves nothing.
private final class CosignedNode: URLProtocol {
    struct State {
        var blockhashSlot: UInt64 = 130       // the slot isBlockhashValid answers at
        var blockhashValid = false
        var listSlot: UInt64 = 130            // the slot getSignaturesForAddress answers at
        var list: [(signature: String, slot: UInt64)] = []
        var indexedLate: [(signature: String, slot: UInt64)] = []   // in the list only finalized: the index behind replay
        var nullList = false
        var noMeta: Set<String> = []          // getTransaction answers these without their outcome
        var transactions: [String: (signatures: [String], err: String?)] = [:]
        var silent = false                    // every call: HTTP 400, the node doesn't answer anything useful
        var v1: Set<String> = []              // v1 transactions: getTransaction refuses them unless asked with version 1
        var rentRate: UInt64 = 5_080          // lamports a byte: an account's rent minimum is (128 + its size) × this
        var balances: [String: UInt64] = [:]
    }

    static let lock = NSLock()
    nonisolated(unsafe) static var states: [String: State] = [:]
    static let installed = URLProtocol.registerClass(CosignedNode.self)

    static func node(_ name: String, _ state: State) -> SolanaRPC {
        _ = installed
        lock.withLock { states[name] = state }
        return SolanaRPC(url: URL(string: "https://\(name).cosigned.stub")!)
    }

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host?.hasSuffix(".cosigned.stub") == true }
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
        let name = String(request.url!.host!.dropLast(".cosigned.stub".count))
        let state = Self.lock.withLock { Self.states[name] ?? State() }
        let call = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any]
        let params = call?["params"] as? [Any] ?? []
        let config = params.last as? [String: Any]
        let least = (config?["minContextSlot"] as? NSNumber)?.uint64Value ?? 0
        let behind = #"{"jsonrpc":"2.0","id":1,"error":{"code":-32016,"message":"Minimum context slot has not been reached"}}"#
        func result(_ json: String) -> String { #"{"jsonrpc":"2.0","id":1,"result":\#(json)}"# }
        let reply: String = switch call?["method"] as? String {
        case "isBlockhashValid":
            least > state.blockhashSlot ? behind : result(#"{"context":{"slot":\#(state.blockhashSlot)},"value":\#(state.blockhashValid)}"#)
        case "getSignaturesForAddress" where state.nullList: result("null")
        case "getSignaturesForAddress":
            least > state.listSlot ? behind : result("[" + (config?["commitment"] as? String == "finalized" ? state.indexedLate + state.list : state.list).map {
                #"{"signature":"\#($0.signature)","slot":\#($0.slot),"err":null,"confirmationStatus":"confirmed","blockTime":1790000000}"#
            }.joined(separator: ",") + "]")
        case "getTransaction" where state.v1.contains(params.first as? String ?? "")
                                    && (config?["maxSupportedTransactionVersion"] as? NSNumber)?.intValue ?? -1 < 1:
            // Mainnet's words, 2026-11-05.
            #"{"jsonrpc":"2.0","id":1,"error":{"code":-32015,"message":"Transaction version (1) is not supported by the requesting client. Please try the request again with the following configuration parameter: \"maxSupportedTransactionVersion\": 1"}}"#
        case "getMinimumBalanceForRentExemption":
            result("\((128 + ((params.first as? NSNumber)?.uint64Value ?? 0)) * state.rentRate)")
        case "getBalance":
            result(#"{"context":{"slot":130},"value":\#(state.balances[params.first as? String ?? ""] ?? 0)}"#)
        case "getTransaction":
            (params.first as? String).flatMap { id in state.transactions[id].map { (id, $0) } }.map { id, tx in
                let meta = state.noMeta.contains(id) ? "null" : #"{"err":\#(tx.err ?? "null")}"#
                return result(#"{"slot":125,"transaction":{"signatures":[\#(tx.signatures.map { "\"\($0)\"" }.joined(separator: ","))],"message":{}},"meta":\#(meta)}"#)
            } ?? result("null")
        case "simulateTransaction": result(#"{"context":{"slot":777},"value":{"err":null,"logs":[],"accounts":[]}}"#)
        default: result("null")
        }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: state.silent ? 400 : 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(reply.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

private let ours = "OurSecondSignature", sponsor = "SponsorsFirstSignature", wallet = "5PkKzEWa11etTestTestTestTestTestTestTestZhb1"
/// The slot of the simulation before signing: its node knew the blockhash then, and nothing signed after it lands at or before it.
private let simulated: UInt64 = 100

private func landed(_ err: String? = nil) -> CosignedNode.State {
    var s = CosignedNode.State()
    s.blockhashValid = true
    s.list = [(sponsor, 125), ("older", 90)]
    s.transactions = [sponsor: ([wallet, ours], err)]
    return s
}

/// Gone: the blockhash is past use at slot 130, and the wallet's transactions since the signing hold none with ours.
private func gone() -> CosignedNode.State {
    var s = CosignedNode.State()
    s.list = [("someoneElses", 125), ("older", 90)]
    s.transactions = ["someoneElses": ([wallet], nil)]
    return s
}

@Test func cosignedFoundIsConfirmedUnderTheSponsorsSignature() async throws {
    let rpc = CosignedNode.node("found", landed())
    let found = try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 5)
    #expect(found.outcome == .confirmed && found.transaction == sponsor)
}

@Test func cosignedFailedOnChainIsFailedWithTheChainsError() async throws {
    let rpc = CosignedNode.node("failed", landed(#"{"InstructionError":[2,{"Custom":6001}]}"#))
    let found = try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 5)
    guard case .failed(let e) = found.outcome else { Issue.record("not failed: \(found)"); return }
    #expect(e.contains("6001") && found.transaction == sponsor)
}

@Test func cosignedGoneAfterTheBlockhashDiesIsExpired() async throws {
    let rpc = CosignedNode.node("gone", gone())
    let found = try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 5)
    #expect(found.outcome == .expired && found.transaction == nil)
}

/// Someone else's v1 transaction newer than ours in the wallet's list (spam mostly: v1 is a fifth of mainnet's since
/// Nov 2026): read with version 1, so ours is found, not waited for till the timeout.
@Test func cosignedReadsV1Neighbours() async throws {
    var s = landed()
    s.list.insert(("spamV1", 127), at: 0)
    s.transactions["spamV1"] = (["SpamSender"], nil)
    s.v1 = ["spamV1"]
    let found = try await CosignedNode.node("v1", s).waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 5)
    #expect(found.outcome == .confirmed && found.transaction == sponsor)
}

/// Rent is the network's word, never a constant: it fell from 6,960 lamports a byte to 5,080 in Sep 2026 and steps
/// down again with Agave 4.4 (SIMD-0437). Kamino's first-deposit rent, measured at 5,080, scales with it.
@Test func rentIsReadFromTheNetwork() async throws {
    #expect(try await CosignedNode.node("rent", .init()).rentMinimum() == 650_240)
    #expect(try await CosignedNode.node("rent", .init()).rentMinimum(bytes: 165) == 1_488_440)
    var step3 = CosignedNode.State()
    step3.rentRate = 2_575
    let kamino = await Executor.kaminoFirstRent(CosignedNode.node("rentStep3", step3))
    #expect(abs(kamino - Decimal(string: "0.0151")!) < Decimal(string: "0.0001")!, "\(kamino)")
    var silent = CosignedNode.State()
    silent.silent = true
    #expect(await Executor.kaminoFirstRent(CosignedNode.node("rentSilent", silent)) == Decimal(string: "0.0298")!)
}

/// A send to an empty account: refused under the network's rent minimum, with that minimum named; 0.0007 SOL, which the
/// old constant (0.00089088) refused, goes on past it (to the blockhash this node doesn't give).
@Test func anEmptyAccountOpensWithTheNetworksRentMinimum() async throws {
    var s = CosignedNode.State()
    s.balances = [wallet: 1_000_000_000]
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("rent-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let executor = Executor(wallet: TradeWallet(solana: wallet, evm: "0x0"), signer: ZeroSigner(), journal: Journal(url: url),
                            rpc: CosignedNode.node("rentSend", s)) { _ in }
    func said(_ sol: String) async -> String {
        do { _ = try await executor.sendSOL(Decimal(string: sol)!, to: "EmptyAccount", label: "Savings"); return "sent" } catch { return error.localizedDescription }
    }
    let short = await said("0.0006")
    #expect(short.contains("at least 0.00065024 SOL"), "\(short)")
    let enough = await said("0.0007")
    #expect(!enough.contains("rent minimum"), "\(enough)")
}

/// A wallet with fewer transactions than one page: the list is its whole history, so nothing older can hide ours.
@Test func cosignedShortHistoryReachesItsStart() async throws {
    var s = gone()
    s.list = [("someoneElses", 125)]
    let found = try await CosignedNode.node("short", s).waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 5)
    #expect(found.outcome == .expired)
}

/// A page of 100 all newer than the signing: ours may be further back, so it's never «expired», only unknown.
@Test func cosignedNeverExpiredWhileThePageDoesntReachTheSigning() async {
    var s = gone()
    s.list = (0..<100).map { ("other\($0)", 200 - UInt64($0)) }
    for i in 0..<100 { s.transactions["other\(i)"] = ([wallet], nil) }
    let rpc = CosignedNode.node("page", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

@Test func cosignedWaitsWhileTheBlockhashLives() async {
    var s = gone()
    s.blockhashValid = true
    let rpc = CosignedNode.node("lives", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

/// A node behind the simulation's slot doesn't know the blockhash yet — too new for it, not past use: its «not valid» is
/// never taken for «expired» (another node of the public pool, the public node after the user's own).
@Test func cosignedBlockhashNodeBehindTheSigningIsNotBelieved() async {
    var s = gone()
    (s.blockhashSlot, s.list) = (95, [("older", 90)])
    let rpc = CosignedNode.node("young", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

/// The list read from a node behind the slot that proved the blockhash dead may not have ours yet: not believed either.
@Test func cosignedListNodeBehindTheExpiryIsNotBelieved() async {
    var s = gone()
    s.listSlot = 120
    let rpc = CosignedNode.node("lagging", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

/// The wallet's index is written after a block is replayed: a confirmed list may not have ours yet, so the expiry is
/// proved from the finalized one.
@Test func cosignedExpiryReadsTheFinalizedIndex() async throws {
    var s = gone()
    s.indexedLate = [(sponsor, 128)]
    s.transactions[sponsor] = ([wallet, ours], nil)
    let found = try await CosignedNode.node("lateIndex", s).waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 5)
    #expect(found.outcome == .confirmed && found.transaction == sponsor)
}

/// No slot from the simulation: nothing bounds what the nodes knew, so never «expired».
@Test func cosignedWithoutTheSimulationsSlotNeverExpires() async {
    var s = gone()
    s.list = [("someoneElses", 125)]   // every transaction readable: only the missing bound stops it
    let rpc = CosignedNode.node("noSlot", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: 0, timeout: 1) }
}

/// A node that answers null for the list hasn't shown the wallet's whole history.
@Test func cosignedNullListIsNotTheWholeHistory() async {
    var s = gone()
    s.nullList = true
    let rpc = CosignedNode.node("nullList", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

/// Ours, but read without its outcome: neither «went through» nor «expired».
@Test func cosignedTransactionWithoutItsOutcomeIsUnread() async {
    var s = landed()
    s.blockhashValid = false
    s.noMeta = [sponsor]
    let rpc = CosignedNode.node("noMeta", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

/// The slot the simulation ran at, the bound the search above starts from.
@Test func simulationReturnsItsSlot() async throws {
    let (_, wire) = try SolanaTransaction.transfer(from: Data(repeating: 7, count: 32), to: Base58.encode(Data(repeating: 9, count: 32)),
                                                   lamports: 1, blockhash: Base58.encode(Data(repeating: 1, count: 32)))
    #expect(try await CosignedNode.node("simulate", .init()).simulate(wire, watch: [], sigVerify: false).slot == 777)
}

/// A transaction since the signing that couldn't be read may be ours: no «expired» until it's read.
@Test func cosignedUnreadTransactionBlocksExpiry() async {
    var s = gone()
    s.transactions = [:]
    let rpc = CosignedNode.node("unread", s)
    await expectUnknown { try await rpc.waitForCosigned(ours, signer: wallet, blockhash: "hash", after: simulated, timeout: 1) }
}

private func expectUnknown(_ wait: () async throws -> (outcome: SolanaRPC.Outcome, transaction: String?)) async {
    do {
        let found = try await wait()
        Issue.record("expected «status unknown», got \(found)")
    } catch {
        #expect(error.localizedDescription.contains("still unknown"))
    }
}

@Test func messageCarriesItsBlockhash() throws {
    let blockhash = Base58.encode(Data(repeating: 1, count: 32))
    let (_, wire) = try SolanaTransaction.transfer(from: Data(repeating: 7, count: 32), to: Base58.encode(Data(repeating: 9, count: 32)),
                                                   lamports: 1, blockhash: blockhash)
    #expect(try SolanaMessage.parse(transaction: wire).blockhash == blockhash)
}

// MARK: - What the journal and the chat get

private func settle(_ name: String, _ state: CosignedNode.State) async throws
    -> (entries: [Journal.Entry], unresolved: [Journal.Entry], went: [Journal.Entry], error: String?) {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("cosigned-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    try journal.append(.init(chain: "solana", id: ours, kind: .signed, note: "Jupiter USDC→SOL, fee paid by JupiterZ's market maker; Jupiter order r1, valid until block 0"))
    let executor = Executor(wallet: TradeWallet(solana: wallet, evm: "0x0"), signer: ZeroSigner(), journal: journal, rpc: CosignedNode.node(name, state)) { _ in }
    var said: String?
    do { try await executor.settleSponsored(ours, blockhash: "hash", after: simulated, jupiter: URLError(.timedOut), timeout: 1) } catch {
        said = error.localizedDescription
        // The swap's caller takes the pools' route after these two: a fill that went out would be swapped a second time.
        #expect(!(error.localizedDescription.hasPrefix("The guard stopped") || error.localizedDescription.hasPrefix("The simulation failed")))
        #expect(!(error is GuardStop))
    }
    return (try journal.entries(), try journal.unresolved(), try journal.wentThrough(), said)
}

@Test func settleSponsoredFoundGoesOnAsTheSwap() async throws {
    let (entries, unresolved, went, error) = try await settle("settleFound", landed())
    #expect(error == nil && unresolved.isEmpty)
    #expect(entries.last?.kind == .confirmed && entries.last?.note == "transaction \(sponsor)")
    // What went through links the sponsor's signature, the one an explorer knows: a failure after it offers no «Try again».
    #expect(went.map(\.id) == [sponsor])
}

@Test func settleSponsoredGoneSaysNothingMoved() async throws {
    let (entries, unresolved, _, error) = try await settle("settleGone", gone())
    #expect(entries.last?.kind == .expired && unresolved.isEmpty)
    #expect(error?.contains("can't land: nothing moved") == true)
}

@Test func settleSponsoredFailedOnChainIsFailed() async throws {
    let (entries, _, _, error) = try await settle("settleFailed", landed(#"{"InstructionError":[2,{"Custom":6001}]}"#))
    #expect(entries.last?.kind == .failed && entries.last?.note.contains("6001") == true)
    #expect(error?.contains("reached the chain and failed") == true)
}

/// The node answers nothing: the entry stays «signed», so the chat says «sent, status unknown» and offers no «Try again»
/// (Workspace.unsettled), with the wallet's page to check — never «failed» on a guess.
@Test func settleSponsoredSilentChainStaysSigned() async throws {
    var s = CosignedNode.State()
    s.silent = true
    let (entries, unresolved, _, error) = try await settle("settleSilent", s)
    #expect(entries.count == 1 && entries.last?.kind == .signed && unresolved.map(\.id) == [ours])
    #expect(error?.contains("solscan.io/account/\(wallet)") == true)
    #expect(entries.allSatisfy { $0.kind != .failed })
}

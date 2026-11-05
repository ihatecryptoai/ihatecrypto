@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import SolanaChain
import Testing
import WalletCore

/// A Solana node with a scripted history: `signatures` for getSignaturesForAddress (nil: HTTP 400, which isn't asked
/// again), and for getTransaction by signature «ok» (a transaction), «null» (the node hasn't it), anything else HTTP 400.
/// It counts each signature's getTransaction and keeps the commitment each read asked for.
nonisolated final class MovesNode: URLProtocol {
    nonisolated(unsafe) static var signatures: [(sig: String, failed: Bool)]? = []
    nonisolated(unsafe) static var transactions: [String: String] = [:]
    nonisolated(unsafe) static var asked: [String: Int] = [:]
    nonisolated(unsafe) static var commitments: [String] = []
    static let owner = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", bonk = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "moves.stub" }
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
        let params = j["params"] as? [Any] ?? []
        Self.commitments.append((params.last as? [String: Any])?["commitment"] as? String ?? "none")
        var result: Any?, error: [String: Any]?
        switch j["method"] as? String {
        case "getSignaturesForAddress":
            result = Self.signatures.map { $0.map { ["signature": $0.sig, "err": $0.failed ? ["InstructionError": [0, "Custom"]] : NSNull()] as [String: Any] } }
        case "getTransaction":
            let sig = params.first as? String ?? ""
            Self.asked[sig, default: 0] += 1
            let version = ((params.last as? [String: Any])?["maxSupportedTransactionVersion"] as? NSNumber)?.intValue ?? -1
            switch Self.transactions[sig] {
            case "v1" where version < 1:   // mainnet's answer to a v1 asked for without version 1 (2026-11-05)
                error = ["code": -32015, "message": "Transaction version (1) is not supported by the requesting client. Please try the request again with the following configuration parameter: \"maxSupportedTransactionVersion\": 1"]
            case "ok", "v1": result = Self.buy(at: sig == "A" ? 1_000 : 2_000)
            case "null": result = NSNull()
            default: result = nil
            }
        default: result = nil
        }
        let reply = error.map { try! JSONSerialization.data(withJSONObject: ["jsonrpc": "2.0", "id": 1, "error": $0]) }
            ?? result.map { try! JSONSerialization.data(withJSONObject: ["jsonrpc": "2.0", "id": 1, "result": $0]) }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: reply == nil ? 400 : 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: reply ?? Data("bad request".utf8))
        client!.urlProtocolDidFinishLoading(self)
    }

    /// 1 BONK in, as getTransaction (jsonParsed) shows it.
    static func buy(at time: Int) -> [String: Any] {
        func bal(_ raw: String) -> [String: Any] { ["mint": bonk, "owner": owner, "uiTokenAmount": ["amount": raw, "decimals": 5]] }
        return ["blockTime": time,
                "meta": ["err": NSNull(), "fee": 5000, "preBalances": [5_000_000_000], "postBalances": [5_000_000_000],
                         "preTokenBalances": [bal("0")], "postTokenBalances": [bal("100000")]],
                "transaction": ["message": ["accountKeys": [["pubkey": owner, "signer": true]]]]]
    }
}

/// A cost from part of a token's history is wrong and was kept till the amount changed: a purchase the node didn't give
/// counted as held before, at the price that day. Now a read the node cut short answers nothing (nil), and what was read
/// stays read, so the next ask reads only what's missing.
@Suite(.serialized) struct SolanaMovesTests {
    let rpc = SolanaRPC(url: URL(string: "https://moves.stub")!)

    func moves(_ m: SolanaMoves, owner: String = MovesNode.owner, signatures: [(sig: String, failed: Bool)]?, transactions: [String: String]) async -> [CostBasis.SolanaMove]? {
        URLProtocol.registerClass(MovesNode.self)
        defer { URLProtocol.unregisterClass(MovesNode.self) }
        (MovesNode.signatures, MovesNode.transactions, MovesNode.commitments) = (signatures, transactions, [])
        return await m.moves(owner: owner, mint: CostBasis.wsol, rpc: rpc)
    }

    @Test func aTransactionNotReadAnswersNothing() async {
        MovesNode.asked = [:]
        #expect(await moves(SolanaMoves(), signatures: [("A", false), ("B", false)], transactions: ["A": "ok", "B": "down"]) == nil)
    }

    @Test func theNextAskReadsOnlyWhatWasMissing() async {
        MovesNode.asked = [:]
        let m = SolanaMoves()
        #expect(await moves(m, signatures: [("A", false), ("B", false)], transactions: ["A": "ok", "B": "down"]) == nil)
        let all = await moves(m, signatures: [("A", false), ("B", false)], transactions: ["A": "ok", "B": "ok"])
        #expect(all?.map(\.at) == [Date(timeIntervalSince1970: 1_000), Date(timeIntervalSince1970: 2_000)])
        // Whichever the first ask took first: A read once (or not at all the first time), B twice.
        #expect(MovesNode.asked == ["A": 1, "B": 2], "\(MovesNode.asked)")
    }

    /// null for a signature the node itself listed: one it doesn't have (another node in the pool), not «no transaction».
    @Test func aListedTransactionNullIsNotRead() async {
        #expect(await moves(SolanaMoves(), signatures: [("A", false), ("B", false)], transactions: ["A": "ok", "B": "null"]) == nil)
    }

    /// A v1 transaction (a fifth of mainnet's since Nov 2026) is read like any other: asked for with version 1. Asked with
    /// 0 the node refused it (-32015), and the coin's P/L was never whole.
    @Test func aV1TransactionIsRead() async {
        let all = await moves(SolanaMoves(), signatures: [("A", false), ("B", false)], transactions: ["A": "ok", "B": "v1"])
        #expect(all?.count == 2, "\(String(describing: all))")
    }

    @Test func signaturesNotReadAnswerNothing() async {
        #expect(await moves(SolanaMoves(), signatures: nil, transactions: [:]) == nil)
    }

    /// A failed transaction counts for nothing, so it's never asked for: the read is whole without it.
    @Test func aFailedTransactionIsNotAsked() async {
        MovesNode.asked = [:]
        let all = await moves(SolanaMoves(), signatures: [("A", false), ("X", true)], transactions: ["A": "ok"])
        #expect(all?.count == 1 && MovesNode.asked["X"] == nil)
    }

    /// A move is one wallet's side of a transaction: after a switch, the other wallet's read of the same one (a send
    /// between the two) is its own, never the first wallet's taken from what was read.
    @Test func aTransactionReadForOneWalletIsReadAgainForAnother() async {
        let m = SolanaMoves(), other = "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin"
        let mine = await moves(m, signatures: [("A", false)], transactions: ["A": "ok"])
        let theirs = await moves(m, owner: other, signatures: [("A", false)], transactions: ["A": "ok"])
        #expect(mine?.first?.change[MovesNode.bonk] == 1)
        #expect(theirs?.count == 1 && theirs?.first?.change[MovesNode.bonk] == nil, "\(String(describing: theirs))")
    }

    /// The portfolio's amounts are read at «confirmed» (Network.swift); history at the node's default «finalized» lagged
    /// them by ~13 s, so a purchase just made was held but not in the history: counted as held before, at the day's
    /// price, and kept till the amount changed again.
    @Test func historyIsReadAtTheCommitmentTheAmountsAre() async {
        _ = await moves(SolanaMoves(), signatures: [("A", false)], transactions: ["A": "ok"])
        #expect(MovesNode.commitments == ["confirmed", "confirmed"], "\(MovesNode.commitments)")
    }

    /// Live (IHATECRYPTO_NET=1, reads only): the public node gives a wallet's whole history, one move for each of its
    /// newest 50 transactions that didn't fail. IHATECRYPTO_OWNER names the wallet (the Dev copy's test wallet, say);
    /// the «abandon … about» one's are mostly failed spam.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
    func theWholeHistoryIsReadOnMainnet() async throws {
        let owner = ProcessInfo.processInfo.environment["IHATECRYPTO_OWNER"] ?? MovesNode.owner
        let node = SolanaRPC(url: SolanaNodes.publicNode)
        let list = try await node.call("getSignaturesForAddress", [owner, ["limit": 50, "commitment": "confirmed"]]) as? [[String: Any]] ?? []
        let ok = Set(list.filter { $0["err"] == nil || $0["err"] is NSNull }.compactMap { $0["signature"] as? String })
        let moves = await SolanaMoves().moves(owner: owner, mint: CostBasis.wsol, rpc: node)
        let read = moves?.count ?? -1
        // A transaction or two may land between the two reads.
        print("live: \(read) moves of \(ok.count) transactions")
        #expect(ok.count > 0 && abs(read - ok.count) <= 2, "\(read) of \(ok.count)")
    }
}

@testable import DeFiKamino
import Foundation
import Testing

/// api.kamino.finance as a test sets it: path → (status, body). A path not set answers 404, as Kamino's «Cannot GET».
private final class KaminoStub: URLProtocol {
    nonisolated(unsafe) static var answers: [String: (Int, String)] = [:]

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "api.kamino.finance" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        let path = request.url!.path
        let (status, body) = Self.answers[path] ?? (404, "Cannot GET \(path)")
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(body.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// A Kamino Vault deposit as the portfolio row in 52f1973 read it: 4.687558 shares of «Allez USDC» at 1.0666 USDC a
/// share. The vault's name and token were read once per launch, and an error answer was kept as «no vaults» until a
/// restart: «Kamino Vault · 5 ?», and «withdraw USDC from kamino» found no position.
@Suite(.serialized) struct KaminoVault {
    static let allez = "A1USdzqDHmw5oz97AkqAGLxEQZfFjASZFuy4T6Qdvnpo"
    static let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    static let wallet = "StubWallet"
    static let metrics = (200, #"{"tokensPerShare":"1.0666","tokenPrice":"1","apy":"0.0509"}"#)

    static func vault(name: String) -> (Int, String) {
        (200, #"{"address":"\#(allez)","state":{"name":"\#(name)","tokenMint":"\#(usdc)","tokenMintDecimals":6}}"#)
    }

    func read(vault: (Int, String), metrics: (Int, String) = metrics) async throws -> [Kamino.VaultPosition] {
        KaminoStub.answers = [
            "/kvaults/users/\(Self.wallet)/positions": (200, #"[{"vaultAddress":"\#(Self.allez)","totalShares":"4.687558"}]"#),
            "/kvaults/\(Self.allez)/metrics": metrics,
            "/kvaults/vaults/\(Self.allez)": vault,
        ]
        URLProtocol.registerClass(KaminoStub.self)
        defer { URLProtocol.unregisterClass(KaminoStub.self) }
        return try await Kamino.vaultPositions(wallet: Self.wallet)
    }

    @Test func aFailedReadIsntKeptUntilARestart() async throws {
        let error = await #expect(throws: KaminoError.self) { try await read(vault: (500, #"{"error":"An internal error occurred"}"#)) }
        #expect(error?.message.contains("internal error") == true, "\(String(describing: error))")
        // Kamino answers again: the same launch reads the vault as it is.
        let p = try await read(vault: Self.vault(name: "Allez USDC"))
        #expect(p.count == 1)
        #expect(p.first?.name == "Allez USDC")
        #expect(p.first?.symbol == "USDC")
        #expect(p.first?.mint == Self.usdc)
        #expect(p.first?.tokens == Decimal(string: "4.687558")! * Decimal(string: "1.0666")!)
    }

    /// An error body is no data, nor a vault answer without its token: never a «?» vault with no mint.
    @Test func anAnswerWithoutTheVaultsTokenIsAnError() async throws {
        for answer in [(401, #"{"error":"Not authorized"}"#), (200, #"{"error":"Not authorized"}"#), (200, #"{"state":{"name":"Allez USDC"}}"#)] {
            await #expect(throws: KaminoError.self, "\(answer)") { try await read(vault: answer) }
        }
    }

    /// Eight of Kamino's vaults have no name: the vault's token is still read.
    @Test func aVaultWithoutANameKeepsItsToken() async throws {
        let p = try await read(vault: Self.vault(name: ""))
        #expect(p.first?.name == "Kamino Vault")
        #expect(p.first?.symbol == "USDC")
        #expect(p.first?.mint == Self.usdc)
    }

    /// A refused metrics read is an error, not a deposit worth $0.
    @Test func aRefusedMetricsReadIsAnError() async throws {
        await #expect(throws: KaminoError.self) {
            try await read(vault: Self.vault(name: "Allez USDC"), metrics: (429, #"{"error":"Too many requests"}"#))
        }
    }

    /// Live (IHATECRYPTO_NET=1, reads only): Kamino's own answer for «Allez USDC», and its 500 for a vault that isn't.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
    func liveVaultByAddress() async throws {
        let v = try await Kamino.vault(Self.allez)
        #expect(v.name == "Allez USDC")
        #expect(v.mint == Self.usdc)
        await #expect(throws: KaminoError.self) { try await Kamino.vault("11111111111111111111111111111111") }
    }
}

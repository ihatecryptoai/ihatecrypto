import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif
import LLMCore
import Testing

/// The browser sign-in's redirect is caught on 127.0.0.1 and read back: the Mac's Network.framework listener, and the
/// plain socket one the desktop app uses on Windows and Linux.
@Test func theSignInRedirectIsCaughtOnTheLoopback() async throws {
    let server = try await LoopbackServer.start(path: "/callback")
    #expect(server.port > 0)
    async let query = server.waitForCallback(timeout: 20)
    let url = URL(string: "http://127.0.0.1:\(server.port)/callback?code=abc&state=xyz")!
    let (data, _) = try await URLSession.shared.data(from: url)
    #expect(String(decoding: data, as: UTF8.self).contains("Signed in"))
    let got = try await query
    #expect(got["code"] == "abc" && got["state"] == "xyz")
}

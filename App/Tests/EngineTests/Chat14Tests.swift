@testable import DesktopEngine
import Foundation
import Testing

/// The chat as the Mac's (audit12 area A): the connect bar says Ollama is too old only after a download said so, not
/// when it's merely behind its newest release. State only: nothing here asks Ollama or GitHub anything.
@MainActor @Suite(.serialized) struct Chat14Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("chat14-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    private var engine: Engine { Self.engine }

    private func sent() throws -> [String: Any] {
        let data = try JSONEncoder().encode(engine.localSetupView)
        return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    @Test func anOllamaOnlyBehindItsNewestIsNotTooOldForTheBar() throws {
        engine.localServers["ollama"] = true
        engine.liveModels["ollama"] = []
        engine.localSetup.ollamaVersion = "0.30.0"
        engine.localSetup.ollamaLatest = "0.35.1"
        // Settings' line offers the update (the Mac's localState) …
        #expect(engine.ollamaState == "outdated")
        // … but the chat's bar keeps «isn't on this PC yet · Download», which updates Ollama first (the Mac's ollamaOutdated).
        #expect(try sent()["ollamaOutdated"] as? Bool == false)
        // A download refused for an old Ollama: now the bar says it's too old.
        engine.localSetup.ollamaOutdated = true
        #expect(try sent()["ollamaOutdated"] as? Bool == true)
        engine.localServers["ollama"] = nil
        engine.liveModels["ollama"] = nil
        engine.localSetup = LocalSetup()
    }
}
